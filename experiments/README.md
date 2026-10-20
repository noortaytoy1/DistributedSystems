# Experiments

Run configurations for the measurements in the report. Each experiment varies one thing and
records the events listed in [src/metrics.rs](../src/metrics.rs).

| ID | Varies                  | Measures                                            | Owner |
|----|-------------------------|-----------------------------------------------------|-------|
| E1 | users × policy          | latency percentiles, throughput, fairness           | B, F  |
| E2 | CPU burn on one node    | share of work per node                              | E     |
| E3 | victim × load           | client-side gap, election time, in-flight outcomes  | C     |
| E4 | silence length × load   | time to pass the rejoin check                       | D     |
| E5 | faults on or off        | elections, messages by type, commit latency         | A     |
| E6 | load-score variant      | latency and fairness under skew                     | B     |

## Rules

- At least 5 runs per cell. Drop the first 30 s of each run as warm-up.
- Client latency is measured on the client clock.
- Each run writes to its own directory, `runs/<experiment>/<cell>/<run>/`, by setting
  `metrics.dir` in that run's config. Raw logs stay out of git (`runs/` is ignored); keep them
  on the lab machines until the report is done.
- Put the config for each cell here, named `<experiment>-<cell>.toml`, so any run can be
  repeated.
