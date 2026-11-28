#[path = "../tests/gie/mod.rs"]
mod gie;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use gie::{AxisTable, Case, Engine, Failure, Outcome, SUITES, SkipReason, gie_files, parse, run};

struct Largest {
    error: f64,
    tolerance: f64,
    at: String,
}

#[derive(Default)]
struct Tally {
    cases: usize,
    passed: usize,
    failed: usize,
    raised: usize,
    first_raised: Option<String>,
    largest: Option<Largest>,
    skipped: BTreeMap<SkipReason, usize>,
}

impl Tally {
    fn add(&mut self, at: &str, case: &Case, outcome: &Outcome) {
        self.cases += 1;
        let failure = match outcome {
            Outcome::Passed => {
                self.passed += 1;
                return;
            }
            Outcome::Skipped(reason) => {
                *self.skipped.entry(*reason).or_default() += 1;
                return;
            }
            Outcome::Failed(failure) => failure,
        };
        self.failed += 1;
        match failure {
            Failure::Outside { error } => {
                let larger = self
                    .largest
                    .as_ref()
                    .is_none_or(|largest| error.total_cmp(&largest.error).is_gt());
                if larger {
                    self.largest = Some(Largest {
                        error: *error,
                        tolerance: case.tolerance,
                        at: at.to_string(),
                    });
                }
            }
            Failure::Raised(_) => {
                self.raised += 1;
                if self.first_raised.is_none() {
                    self.first_raised = Some(format!("{at}: {failure}"));
                }
            }
            Failure::DidNotFail => {}
        }
    }

    fn print(&self, title: &str) {
        println!(
            "{title}: {} cases, {} passed, {} failed, {} skipped",
            self.cases,
            self.passed,
            self.failed,
            self.cases - self.passed - self.failed
        );
        if let Some(largest) = &self.largest {
            println!(
                "    largest error {:.3e} at {}, tolerance {:.3e}",
                largest.error, largest.at, largest.tolerance
            );
        }
        if let Some(first) = &self.first_raised {
            println!("    {} raised, first at {first}", self.raised);
        }
        if !self.skipped.is_empty() {
            let reasons: Vec<String> = self
                .skipped
                .iter()
                .map(|(reason, count)| format!("{} {count}", reason.label()))
                .collect();
            println!("    skipped: {}", reasons.join(", "));
        }
    }
}

fn read_suite(data: &Path, directory: &str) -> Vec<(String, Vec<Case>)> {
    let names = gie_files(&data.join(directory)).unwrap_or_else(|e| panic!("{e}"));
    names
        .into_iter()
        .map(|name| {
            let file = format!("{directory}/{name}");
            let text =
                fs::read_to_string(data.join(&file)).unwrap_or_else(|e| panic!("{file}: {e}"));
            let cases = parse(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
            (file, cases)
        })
        .collect()
}

fn print_report(data: &Path, axes: &AxisTable, engine: Engine) {
    for (title, directory) in SUITES {
        let suite = read_suite(data, directory);
        let mut total = Tally::default();
        for (file, cases) in &suite {
            let mut tally = Tally::default();
            for case in cases {
                let outcome = run(case, axes, engine);
                tally.add(&format!("line {}", case.line), case, &outcome);
                total.add(&format!("{file} line {}", case.line), case, &outcome);
            }
            let note = if file.ends_with(".failing") {
                " (PROJ fails this file)"
            } else {
                ""
            };
            tally.print(&format!("{file}{note}"));
        }
        println!();
        total.print(&format!("{title} total over {} files", suite.len()));
        println!();
    }
}

fn print_manifest(data: &Path, axes: &AxisTable, engine: Engine) {
    for (_, directory) in SUITES {
        for (file, cases) in read_suite(data, directory) {
            for case in &cases {
                if matches!(run(case, axes, engine), Outcome::Passed) {
                    println!("{file} {}", case.line);
                }
            }
        }
    }
}

fn main() {
    let usage = "usage: gie_report <data directory> [--manifest] [--candidate-operations]";
    let mut args = std::env::args().skip(1);
    let data = PathBuf::from(args.next().expect(usage));
    let axes = AxisTable::load(&data.join("crs_axes.tsv")).unwrap_or_else(|e| panic!("{e}"));
    let mut manifest = false;
    let mut engine = Engine::Transform;
    for flag in args {
        match flag.as_str() {
            "--manifest" => manifest = true,
            "--candidate-operations" => engine = Engine::CandidateOperations,
            other => panic!("unknown argument {other:?}, {usage}"),
        }
    }
    if manifest {
        print_manifest(&data, &axes, engine);
    } else {
        print_report(&data, &axes, engine);
    }
}
