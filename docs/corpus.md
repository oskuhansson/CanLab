# The corpus format

Exam problems as machine-readable rows, scored by `canlab corpus` — **not implemented yet** (W0.4); this
file is its specification.

## 1. The row format

One TOML file per problem at `tests/cases/<subject>/<id>.toml`; `subject` is `math` or `physics`. Write
LaTeX in TOML literal strings (`'…'`), so a backslash is itself.

| Field | Meaning |
|---|---|
| `id`, `subject` | `id` is the file stem and unique; `subject` is `math` or `physics` and equals the directory. A mismatch or duplicate is a load error. |
| `capability` | The report's grouping tag: lowercase, dotted (`solve.quadratic`), first segment the area, rest the operation. A new spelling is a new capability; reuse the tag. |
| `source` | A citation, never the text: `'<URL> — <year>, uppgift <n>'`, `'hf:<repo> <config>#<row-key>'`, or `'docs/corpus.md §6'` for the format's own refusal example. |
| `document` | The student's LaTeX: `\|` lines, `\|\|` a system's equations, a definition (`f(x) = x^2 + 1`) an ordinary line. |
| `expect` | The expected outcome: exactly one of `exact`, `numeric` (+ `tolerance`, `unit`, `digits`), `refused`, in an `expect` table (dotted keys are fine). |
| `note` | Optional: what the row tests and what it leaves out. |

The row is a restricted TOML subset — top-level string and number keys, one `expect` table (a `[expect]`
header or dotted `expect.` keys), `#` comments — and anything outside it is a load error.

Definitions resolve first; the last non-definition line decides: equation solved, expression evaluated,
`||` solved as a system. A single equation must name one unknown — two is a load error. The editor's field
code builds the document; the backend is the GUI's choice, recorded in the report.

**`expect`** has three forms. `exact` is the answer's canonical LaTeX — `Solutions::to_latex` for an
equation (`\left\{0, 8\right\}`, `\varnothing`, `\mathbb{R}`, `\frac{\pi}{6} + 2k\pi`, `(-1, 3]`),
`to_latex` for an expression (`63`), `x = 3; y = 5` for a system, matched by variable name. A finite set
is compared as a set, other forms as text with whitespace collapsed; `0.5` is not `\frac{1}{2}`.
`numeric` is a number with `tolerance` (absolute, in `unit`), `unit` (physics only) and `digits` (both
sides rounded half-even). No answer carries a quantity until W15, so the unit is recorded, **not
checked**, and the row is reported `pass (unit unchecked)`. `refused` is the reason text: the row passes
when the run refuses and its reason contains it (case-insensitive, whitespace collapsed); name the
construct (`division by zero`), not the mood. A row that breaks these rules is a **load error**: reported,
not run, and the run exits 2.

## 2. Why the statement is not committed

A problem is prose, figures and tables; all belong to the exam board or the dataset, and none is what the
parser sees. The row holds the model — the equations in CanLab's LaTeX — plus the answer and the citation.
Copying the statement would commit copyrighted text and layout and make the row depend on a figure. The
engineering reason matches: the modelling step is the student's (C2), so a corpus handed the equation has
done the exam's real work, and **this corpus cannot measure modelling** — only what follows it. Even where
the source carries LaTeX (the 2025 mathematics paper prints it in each formula's MathJax title), copying
it would commit the statement and skip the model.

## 3. Where rows come from

**Coverage** is what the tool must do (AbiMix); **stress** is what it must survive. A row says which by
its `source` — an AbiMix URL, an `hf:` id, or this file for a format example — and the report counts them
apart, never summing them into one percentage. A corpus of only stress rows measures the wrong thing.

| Source | What it contains | Use |
|---|---|---|
| **AbiMix mathematics** — [svenska.yle.fi/abimix/matematik](https://svenska.yle.fi/abimix/matematik) | Exam articles 2018–autumn 2026, short and long syllabus, each linking the paper and the board's *beskrivningar av goda svar*. 2023 onwards are HTML; the 2018 papers are Google Drive files, so those years are transcribed by hand. | **Coverage.** |
| **AbiMix physics** — [svenska.yle.fi/abimix/fysik](https://svenska.yle.fi/abimix/fysik), [2025 papers](https://yle.fi/a/7-10070812) | The index shows 2009–autumn 2026. The 2025 spring paper has 11 tasks, 7 to answer; most carry figures or tables, and a good answer gives a formula and a number with a unit. | **Coverage**, once W15 exists: a physics answer is a quantity. |
| **OlympiadBench** — [dataset](https://huggingface.co/datasets/Hothan/OlympiadBench), [arXiv:2402.14008](https://arxiv.org/abs/2402.14008) | 8,476 problems in 18 Hugging Face configs (their counts sum to the paper's 8,476), English and Chinese, with `final_answer`, `unit`, `answer_type`, `difficulty` and up to nine images. Apache-2.0. | **Stress.** Harder than the Finnish exam. |
| **UGPhysics** — [dataset](https://huggingface.co/datasets/UGPhysics/ugphysics), [arXiv:2502.00334](https://arxiv.org/abs/2502.00334) | 5,520 problems in 13 configs, English and Chinese, with `answers`, `answer_type`, `unit`, `is_multiple_answer`, `level`. CC BY-NC-SA 4.0, the strictest licence here. | **Stress**, and the bridge to the CanSat calculations. |
| **SciBench** — [dataset](https://huggingface.co/datasets/xw27/scibench), [project](https://scibench-ucla.github.io/) | 692 problems from 10 textbook sets, MIT, with `answer_latex`, `answer_number`, `unit`, `problemid`. Chemistry-heavy; its `class` set gave the physics row below. | **Stress**, and numeric answers with units. |

## 4. Fetching, not vendoring

`canlab corpus fetch [name]` downloads the cited datasets (`olympiadbench`, `ugphysics`, `scibench`; no
name means all three) into `%LOCALAPPDATA%\canlab\corpus\<dataset>\<revision>\`, with a `manifest.toml` of
the URL, revision, bytes and SHA-256 received; it never writes into `tests/cases/`. There is no usable
Python here — the only `python.exe` on the path is a 0-byte Store stub that fails to start — so the fetch
is HTTPS plus a small JSON parse, not `datasets`. UGPhysics's English set is 13 JSONL files, SciBench 10
JSON. **OlympiadBench is the exception:** its Hugging Face copy is parquet, so the fetch takes the first page
(up to 100 rows) of each config through the datasets-server JSON API — a sample, not the benchmark. With no
network, `fetch` exits 2 with `network unavailable: <the OS error>` and leaves the cache as it was; rows
are self-contained, so `run` works offline and only `--update` needs the network.

## 5. The run contract

```text
canlab corpus run [--tag X] [--batch N] [--update]
```

It reads every row, validates it, runs it, prints one line per row (status, file, expected, got) and
writes `docs/abimix-coverage.md` — the plan's name; it covers the stress rows too.

- **`--tag X`** takes capabilities equal to `X` or starting `X.`; no match exits 2.
- **`--batch N`** sends N rows to a child (`canlab corpus exec`) and reads one JSON result per row. A child
  that crashes, exits non-zero or misses its deadline loses **its batch**: those rows fail with the exit
  code or timeout and the parent starts the next batch, so one crash costs one batch, not the run. N
  defaults to 50, the deadline to 120 s — placeholders until a slow row is measured.
- **`--update`** re-fetches the cited datasets and checks each `hf:` row upstream: the id must exist, and
  a comparable answer column must agree with `expect`. Drift and missing ids fail the run; without the flag
  there is no network and no provenance check, and the report says `provenance unchecked`.
- **Exit codes:** `0` all passed; `1` a row failed — a wrong expected value lands here, and the report
  names the file and both answers; `2` the run could not be trusted (a load error, no row matched,
  `--update` could not check). A refusal row passes when the run refuses for the named reason and fails
  when the tool answers; refusals are counted apart from coverage, because declining honestly is not
  doing the capability.

The report is generated, never edited: per capability, coverage rows, stress rows, passes, failures and
`unit unchecked` count, then a failure table of file, expected and got; an absent capability has **no
row**, which is not 0%.

## 6. Five rows

```toml
# tests/cases/math/2023-kevat-N-1-4.toml
id = "2023-kevat-N-1-4"
subject = "math"
capability = "solve.quadratic"
source = "https://svenska-content.ylestatic.fi/abimix/2023/var/N_sv/index.html — 2023, uppgift 1.4"
document = 'x^2 = 8x'
expect.exact = '\left\{0, 8\right\}'
# tests/cases/math/2023-kevat-N-2.toml
id = "2023-kevat-N-2"
subject = "math"
capability = "solve.linear-system"
source = "https://svenska-content.ylestatic.fi/abimix/2023/var/N_sv/index.html — 2023, uppgift 2"
document = 'x + y = 8 || 2x - y = 1'
expect.exact = 'x = 3; y = 5'
# tests/cases/math/2025-kevat-M-1-5.toml
id = "2025-kevat-M-1-5"
subject = "math"
capability = "calc.definite-integral"
source = "https://svenska-content.ylestatic.fi/abimix/2025/var/M_sv/ — 2025, uppgift 1.5"
document = '\int_1^4 3x^2 dx'
expect.exact = "63"
# tests/cases/physics/scibench-class-2-26.toml
id = "scibench-class-2-26"
subject = "physics"
capability = "units.sigfigs"
source = "hf:xw27/scibench class#Problem 2.26"
document = '\sqrt{\frac{2 \cdot 4^2}{6}}'
note = "The spring model: ½mv² = ½kx² with m = 2 kg, v = 4 m/s, k = 6 N/m, so x = sqrt(mv²/k). Prose not copied."
expect.numeric = 2.3
expect.tolerance = 0.05
expect.unit = "m"
expect.digits = 2
# tests/cases/math/input-division-by-zero.toml
id = "input-division-by-zero"
subject = "math"
capability = "input.malformed"
source = "docs/corpus.md §6 — a contract row, not a problem"
document = '\frac{1}{0}'
expect.refused = "division by zero"
```

These are correct rows, not necessarily passing ones: the quadratic and system are within today's native
solver, the integral needs W3.1, the physics magnitude would pass but its unit is unchecked until W15, and
the refusal is already true today. The answers are 63 = 4³ − 1³, x = 3 and y = 5, and
sqrt(2·4²/6) = 2.309… (2.3 to two significant figures); the quadratic task asks only for the larger root.

## 7. Writing a row by hand

Open the task and its *beskrivning av goda svar* (for an `hf:` source, its upstream row), and put the URL
or id, year and item in `source` first. Type the model into the editor as a student would — `|` between
lines, `||` between a system's equations, definitions on their own lines — and check the field shows the
equation the task means, not a lookalike. Read the answer from the good answer, not from the CAS, and write
it in the canonical form above. Then **run it, and check the expected value is what the CAS actually
produces before you trust the row**; when the two differ, decide which is wrong — by hand or against the
good answer — and fix that one, never the expectation. Finish with a `note` saying what the row tests and
what it leaves out: a row that silently drops half a task inflates coverage.