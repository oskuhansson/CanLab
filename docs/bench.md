# Benchmarks

Generated 2026-11-11 15:22:27 UTC by `canlab bench` on Intel64 Family 6 Model 186 Stepping 3, GenuineIntel, 8 logical processors, x86_64 target (read from the OS) — built in the release profile.

Wall-clock measurements, best of N, taken in this process on this machine. They are not comparable across machines. A measurement that could not be taken reads `n/a`, never `0`.

| measurement | value | detail |
|---|---:|---|
| implicit tracer, 240×240, `x^2 + y^2 = 1` | 2.18 ms/rebuild | best of 20 rebuilds on [-2, 2]²; 469 points on the curve |
| `Program::eval`, 20-node expression | 50.5 ns/call | best of 5 × 1000000 calls |
| `Program::eval_batch`, one row | 49.0 ns/row | best of 5 × 1000000 rows |
| `canlab_plot::sample`, 2000-point range | 0.182 ms | best of 20 samples; 2000 points |
