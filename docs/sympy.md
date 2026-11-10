# The SymPy backend

CanLab has two solvers behind one trait:

| | needs | does |
|---|---|---|
| **native** | nothing | exact rationals; linear and quadratic equations in one variable; polynomial calculus; Gaussian elimination for linear systems of any size |
| **SymPy** | Python 3 and the `sympy` package | general simplify, integrate, limit, series, and `solve` beyond polynomials |

The native backend is always available, so the app never depends on a working
Python. SymPy is found **when it is there**, and the Variables panel says which one
is in use.

## Installing it

SymPy is a Python library, so this is a packaging problem rather than a coding one.
There is no bundled interpreter: a bundled runtime is a distribution question, and
for a project at this scale the honest answer is "install Python".

**Windows**

1. Install Python 3 from [python.org](https://www.python.org/downloads/windows/), or
   `winget install Python.Python.3.12`. Tick *Add python.exe to PATH*.
2. Install SymPy:
   ```
   py -3 -m pip install sympy
   ```
3. Check it:
   ```
   canlab solver --probe
   ```

> `python.exe` and `python3.exe` in `%LOCALAPPDATA%\Microsoft\WindowsApps` are
> Microsoft Store *aliases*, not interpreters, until the Store app is installed.
> They take seconds to fail, which is why CanLab tries `py -3` first and gives each
> candidate at most 400 ms to answer `--version`.

**macOS**

```
brew install python
python3 -m pip install sympy
```

**Linux**

```
sudo apt install python3 python3-sympy     # Debian/Ubuntu
sudo dnf install python3-sympy             # Fedora
python3 -m pip install sympy               # anywhere, in a venv if you prefer
```

## Pointing CanLab at an interpreter

Two environment variables skip the search entirely:

| variable | meaning |
|---|---|
| `CANLAB_PYTHON` | the interpreter, with any launcher arguments: `py -3`, `/opt/py/bin/python3` |
| `CANLAB_WORKER` | the path to `worker.py`, when it is not beside the executable or in the checkout |

And one chooses the backend:

| variable | meaning |
|---|---|
| `CANLAB_SOLVER=native` | always use the native backend, even if SymPy is installed |
| `CANLAB_SOLVER` unset | use SymPy when it is found, native otherwise |

## Checking it

```
canlab solver            # what was found, or exactly what is missing
canlab solver --probe    # start the worker and make it answer one question
```

`--probe` is the difference between "the files exist" and "it works": it starts
the worker, which imports SymPy, and round-trips a request through the real
protocol. Its exit status is non-zero when that fails, so it can be used in a
script.

The GUI makes the same choice on its own worker thread, so a slow search never
blocks a frame. If SymPy is missing, the panel still says *native* and the note
explains why — a hover away, not a modal.

## What the worker is

`crates/canlab-solver/python/worker.py`, one line of JSON in and one line out:

```
{"id": 1, "op": "integrate", "expr": {"Pow": {"base": {"Sym": "x"}, "exp": {"Num": "2"}}}, "var": "x"}
{"id": 1, "ok": {"Mul": [{"Num": "1/3"}, {"Pow": {"base": {"Sym": "x"}, "exp": {"Num": "3"}}}]}}
```

* A declined request answers `{"id": 1, "err": "not_implemented"}`. That is a
  **normal answer**: the UI shows "declined", never an error.
* Anything else goes out as `{"err": "TypeError: ..."}`, and the worker is retired
  rather than reused: a late answer on a corrupt pipe would be read as the answer
  to the next request.
* The parent enforces a 30-second deadline by killing the child. SymPy cannot be
  interrupted portably, so the process boundary *is* the timeout.
* Only stdout is protocol. A stray `print` in library code corrupts the channel,
  which is why every answer is checked against the id it asked with.

## Testing without Python

The protocol is tested in memory: `session.rs` is generic over `BufRead`/`Write`,
so `sympy.rs`'s tests drive the whole request/response path from a `Cursor` with no
interpreter anywhere near it. Discovery is tested for both halves being present and
for each way it can be missing. The worker itself is the one part that cannot be
tested on a machine without Python — which is exactly why `canlab solver --probe`
exists, so that the first thing a user does is confirm it.
