#!/usr/bin/env python3
"""SymPy worker for the canlab-solver JSON-lines protocol.

The Rust side (``crates/canlab-solver/src/sympy.rs``) spawns this file as

    python3 path/to/worker.py

writes one request per line to stdin and reads one response per line from
stdout. Nothing else may be written to stdout: a stray ``print`` from library
code corrupts the channel, which is why the Rust reader checks every answer
against the id it asked with. Diagnostics belong on stderr, although the Rust
side currently discards it.

Requests
--------

    {"id": 1, "op": "simplify",  "expr": <expr>}
    {"id": 2, "op": "diff",      "expr": <expr>, "var": "x"}
    {"id": 3, "op": "integrate", "expr": <expr>, "var": "x"}
    {"id": 4, "op": "solve",     "eq":   <expr>, "var": "x"}
    {"id": 5, "op": "limit",     "expr": <expr>, "var": "x", "at": <expr>}
    {"id": 6, "op": "series",    "expr": <expr>, "var": "x", "at": <expr>,
                                 "order": 4}
    {"id": 7, "op": "solve_all", "eq":   <expr>, "var": "x"}

Responses
---------

    {"id": 1, "ok": <expr>}          # most ops
    {"id": 4, "ok": [<expr>, ...]}   # solve
    {"id": 7, "ok": {"kind": "none"}}                    # solve_all:
    {"id": 7, "ok": {"kind": "all"}}                     #   no solution,
    {"id": 7, "ok": {"kind": "values", "values": [...]}} #   every number,
                                                         #   a finite set
    {"id": 1, "err": "not_implemented"}
    {"id": 1, "err": "ValueError: ..."}

``err: "not_implemented"`` is a *normal* answer. The CAS renders the
unevaluated expression instead of pretending it knows the answer. In
particular, SymPy returns an unevaluated ``Integral`` for integrals it cannot
do; that is encoded as an ``Integral`` node in ``ok`` rather than as an error,
so the caller keeps the integrand.

The tagged AST is the one in ``crates/canlab-solver/src/json.rs``:

    {"Num": "3/2"} | {"Num": "2"} | {"Num": "1.5"}
    {"Sym": "x"}
    {"Add": [...]} | {"Mul": [...]}
    {"Pow": {"base": ..., "exp": ...}}
    {"Fn": {"name": "sin", "args": [...]}}
    {"Integral": {"body": ..., "var": "x"}}

Timeouts are enforced by the parent process, which kills this one. SymPy has
no portable way to interrupt a running ``integrate``, so a request that takes
too long is simply terminated.
"""

import json
import sys
import traceback

import sympy as sp


#: The names the CAS core uses for `Fn` nodes, mapped onto the SymPy callables
#: that actually differentiate and integrate.
#:
#: ``sp.Function("sin")`` is *not* ``sp.sin``: it builds an undefined function
#: that happens to print as ``sin(x)`` and computes nothing, so ``diff`` would
#: hand back an unevaluated ``Derivative``. The table is the difference between
#: a worker that works and one that looks like it does.
BUILTIN_FUNCTIONS = {
    "sin": sp.sin,
    "cos": sp.cos,
    "tan": sp.tan,
    "cot": sp.cot,
    "sec": sp.sec,
    "csc": sp.csc,
    "asin": sp.asin,
    "acos": sp.acos,
    "atan": sp.atan,
    "sinh": sp.sinh,
    "cosh": sp.cosh,
    "tanh": sp.tanh,
    "asinh": sp.asinh,
    "acosh": sp.acosh,
    "atanh": sp.atanh,
    "exp": sp.exp,
    "log": sp.log,
    "ln": sp.log,
    "sqrt": sp.sqrt,
    "cbrt": sp.cbrt,
    "Abs": sp.Abs,
    "abs": sp.Abs,
    "sign": sp.sign,
}


# ---------------------------------------------------------------------------
# Tagged AST -> SymPy
# ---------------------------------------------------------------------------

def load(node):
    """Build a SymPy object from one tagged AST node."""
    if not isinstance(node, dict) or len(node) != 1:
        raise ValueError("expression must be an object with exactly one tag: %r" % (node,))

    tag, body = next(iter(node.items()))

    if tag == "Num":
        return load_number(body)
    if tag == "Sym":
        return sp.Symbol(body)
    if tag == "Add":
        return sp.Add(*[load(item) for item in body])
    if tag == "Mul":
        return sp.Mul(*[load(item) for item in body])
    if tag == "Pow":
        return sp.Pow(load(body["base"]), load(body["exp"]))
    if tag == "Fn":
        return load_function(body["name"], [load(item) for item in body["args"]])
    if tag == "Integral":
        return sp.Integral(load(body["body"]), sp.Symbol(body["var"]))
    raise ValueError("unknown expression tag %r" % (tag,))


def load_function(name, args):
    """Rebuild a named function application, preferring SymPy's own."""
    known = BUILTIN_FUNCTIONS.get(name)
    if known is not None:
        return known(*args)
    # Anything else stays an undefined function: the CAS may print it, but the
    # backend will decline to differentiate or integrate it.
    return sp.Function(name)(*args)


def load_number(text):
    """Parse the text of a ``{"Num": ...}`` node."""
    if not isinstance(text, str):
        raise ValueError("Num must hold a string, got %r" % (text,))

    if "/" in text:
        numerator, _, denominator = text.partition("/")
        return sp.Rational(int(numerator), int(denominator))
    if text in ("NaN", "nan"):
        return sp.nan
    if text in ("inf", "Infinity", "+inf", "+Infinity"):
        return sp.oo
    if text in ("-inf", "-Infinity"):
        return -sp.oo
    if "." in text or "e" in text or "E" in text:
        return sp.Float(text)
    return sp.Integer(int(text))


# ---------------------------------------------------------------------------
# SymPy -> tagged AST
# ---------------------------------------------------------------------------

def dump(expr):
    """Serialise a SymPy object back to one tagged AST node.

    Raises ``NotImplementedError`` for anything the CAS has no node for, such
    as an unevaluated ``Derivative`` or a constant like ``pi``. Declining is
    always preferable to silently approximating.
    """
    expr = sp.sympify(expr)

    if isinstance(expr, sp.Integral):
        return {
            "Integral": {
                "body": dump(expr.function),
                "var": dump_variable(expr.variables[0]),
            }
        }
    if isinstance(expr, sp.Integer):
        return {"Num": str(int(expr))}
    if isinstance(expr, sp.Rational):
        return {"Num": "%d/%d" % (int(expr.p), int(expr.q))}
    if isinstance(expr, sp.Float):
        return {"Num": float_text(float(expr))}
    if isinstance(expr, sp.Symbol):
        return {"Sym": str(expr.name)}
    if expr.is_Add:
        return {"Add": [dump(arg) for arg in expr.args]}
    if expr.is_Mul:
        return {"Mul": [dump(arg) for arg in expr.args]}
    if expr.is_Pow:
        return {"Pow": {"base": dump(expr.base), "exp": dump(expr.exp)}}
    if expr.is_Function:
        return {
            "Fn": {
                "name": type(expr).__name__,
                "args": [dump(arg) for arg in expr.args],
            }
        }
    raise NotImplementedError("cannot represent %r in the tagged AST" % (expr,))


def dump_variable(expr):
    """The name of an integration variable."""
    if isinstance(expr, sp.Symbol):
        return str(expr.name)
    raise NotImplementedError("integration variable must be a symbol, got %r" % (expr,))


def dump_solutions(equation, variable):
    """Answer ``solve_all`` with a tagged solution set.

    ``solveset`` is the only SymPy entry point that promises a *set*: it returns
    every solution of the equation over the reals, or an unevaluated
    ``ConditionSet`` it declines to name. ``solve`` returns a list, and for a
    trigonometric equation that list is the principal values rather than the
    family, so relabelling it as the complete answer would be a lie.

    A set this worker can name is encoded: the empty set, the whole real line,
    and a finite set of values the tagged AST can represent. Families and
    intervals wait for the workstreams that add them on the Rust side; until
    then they are declined, which is a normal answer.
    """
    solutions = sp.solveset(equation, variable, domain=sp.S.Reals)
    if solutions == sp.S.EmptySet:
        return {"kind": "none"}
    if solutions == sp.S.Reals:
        return {"kind": "all"}
    if isinstance(solutions, sp.FiniteSet):
        return {"kind": "values", "values": [dump(value) for value in solutions]}
    raise NotImplementedError("cannot encode the solution set %r" % (solutions,))


def float_text(value):
    """Decimal text that the Rust decoder will read back as a float."""
    if value != value:  # NaN
        return "NaN"
    if value == float("inf"):
        return "inf"
    if value == float("-inf"):
        return "-inf"
    text = repr(value)
    if "." not in text and "e" not in text and "E" not in text:
        text += ".0"
    return text


# ---------------------------------------------------------------------------
# Dispatch
# ---------------------------------------------------------------------------

def symbol(request, key):
    name = request.get(key)
    if not isinstance(name, str):
        raise ValueError("request field %r must be a symbol name" % (key,))
    return sp.Symbol(name)


def required(request, key):
    if key not in request:
        raise ValueError("request is missing %r" % (key,))
    return request[key]


def handle(request):
    """Run one request and return its ``ok`` value."""
    op = request.get("op")

    if op == "simplify":
        return dump(sp.simplify(load(required(request, "expr"))))

    if op == "diff":
        return dump(sp.diff(load(required(request, "expr")), symbol(request, "var")))

    if op == "integrate":
        # SymPy answers an integral it cannot do with an unevaluated
        # `Integral`; `dump` encodes that faithfully instead of raising.
        return dump(sp.integrate(load(required(request, "expr")), symbol(request, "var")))

    if op == "solve":
        equation = load(required(request, "eq"))
        solutions = sp.solve(equation, symbol(request, "var"))
        return [dump(solution) for solution in solutions]

    if op == "solve_all":
        return dump_solutions(load(required(request, "eq")), symbol(request, "var"))

    if op == "limit":
        return dump(
            sp.limit(
                load(required(request, "expr")),
                symbol(request, "var"),
                load(required(request, "at")),
            )
        )

    if op == "series":
        order = int(required(request, "order"))
        expansion = sp.series(
            load(required(request, "expr")),
            symbol(request, "var"),
            load(required(request, "at")),
            order,
        )
        # Drop the O(...) term: the CAS has no node for it and the caller asked
        # for a truncated polynomial in the first place.
        return dump(expansion.removeO())

    raise ValueError("unknown op %r" % (op,))


# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------

def main():
    # Deep-ish trees are normal in a CAS; the default limit trips on long
    # polynomial sums.
    sys.setrecursionlimit(20000)

    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue

        request = None
        request_id = None
        try:
            request = json.loads(line)
            request_id = request.get("id")
            result = handle(request)
            reply = {"id": request_id, "ok": result}
        except NotImplementedError:
            reply = {"id": request_id, "err": "not_implemented"}
        except Exception as exc:  # noqa: BLE001 - the wire must always answer
            # stdout is the protocol; the traceback goes to stderr, where the
            # parent can be changed to show it when debugging.
            traceback.print_exc(file=sys.stderr)
            reply = {
                "id": request_id,
                "err": "%s: %s" % (type(exc).__name__, exc),
            }

        sys.stdout.write(json.dumps(reply, separators=(",", ":")) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
