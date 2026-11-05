# Phase 1 report (LaTeX)

The submitted Phase 1 report. Why it is LaTeX and lives here: D266 in [../design-log.md](../design-log.md).

```text
main.tex            preamble, title page, abstract, references
sections/*.tex      one file per section, in the order main.tex lists them
figures/*.png       the six measured charts (from analysis/results/exp8h/) and one Raft-era chart
```

Every other figure is drawn in TikZ or pgfplots inside the section files, so it can be edited as text.
`% CHECK` and `% CONFIRM` comments mark lines a person should confirm before submitting.

## On Overleaf

1. Download this folder: on GitHub, open `docs/report/` on the branch, or zip it locally
   (`cd docs && zip -r report.zip report`).
2. In Overleaf: **New Project, Upload Project**, and pick the zip.
3. **Menu, Compiler:** pdfLaTeX (the default) works. **Main document:** `main.tex`.
4. Press **Recompile**. The first compile takes a while, for TikZ and pgfplots.

## Locally

```bash
latexmk -pdf main.tex          # with TeX Live
tectonic -X compile main.tex   # or Tectonic, which fetches packages itself
```

## Where the numbers come from

Nothing here is measured anew. Results are copied from [../phase1-report.md](../phase1-report.md) (generated from
the logs), [../../analysis/results/RESULTS.md](../../analysis/results/RESULTS.md) (the Raft-era runs),
[../shared-log.md](../shared-log.md) and the design log. If a result changes there, change it here too.
