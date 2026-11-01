//! Background evaluation: a dedicated thread owns the interpreter's source
//! cache (it is not `Send`), runs `.scad` programs and meshes the result,
//! so the UI never blocks. A new job cancels the one in progress.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use eframe::egui;
use mesher::{RenderOptions, RenderOutput};
use scad_eval::{EvalOptions, Evaluator, Level, Message, SourceCache};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Echo,
    Warning,
    Error,
    Trace,
}

/// One console line, formatted like OpenSCAD's output.
#[derive(Debug, Clone)]
pub struct ConsoleLine {
    pub kind: LineKind,
    pub text: String,
}

/// Outcome of evaluating and meshing a program.
pub struct JobResult {
    pub generation: u64,
    pub path: PathBuf,
    pub console: Vec<ConsoleLine>,
    /// `None` when evaluation or meshing failed or was cancelled.
    pub output: Option<RenderOutput>,
    /// Files read by the program (main file, includes, libraries), with
    /// their modification time when the job started, for auto-reload.
    pub dependencies: Vec<(PathBuf, Option<SystemTime>)>,
    pub eval_time: Duration,
    pub mesh_time: Duration,
}

struct Job {
    generation: u64,
    path: PathBuf,
    cancel: Arc<AtomicBool>,
}

pub struct Worker {
    jobs: Sender<Job>,
    pub results: Receiver<JobResult>,
    generation: u64,
    current_cancel: Option<Arc<AtomicBool>>,
}

impl Worker {
    pub fn spawn(ctx: egui::Context) -> Self {
        let (job_tx, job_rx) = std::sync::mpsc::channel::<Job>();
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("scad-worker".into())
            .spawn(move || {
                let cache = SourceCache::new();
                while let Ok(mut job) = job_rx.recv() {
                    // Only the most recent request matters.
                    while let Ok(newer) = job_rx.try_recv() {
                        job = newer;
                    }
                    let result = run_job(&job, &cache);
                    if result_tx.send(result).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("cannot start the evaluation thread");
        Self {
            jobs: job_tx,
            results: result_rx,
            generation: 0,
            current_cancel: None,
        }
    }

    /// Queues a (re)evaluation and returns its generation number.
    pub fn submit(&mut self, path: PathBuf) -> u64 {
        if let Some(cancel) = self.current_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.generation += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        self.current_cancel = Some(cancel.clone());
        let _ = self.jobs.send(Job {
            generation: self.generation,
            path,
            cancel,
        });
        self.generation
    }
}

pub fn modified_time(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn run_job(job: &Job, cache: &SourceCache) -> JobResult {
    let mut result = JobResult {
        generation: job.generation,
        path: job.path.clone(),
        console: Vec::new(),
        output: None,
        dependencies: vec![(job.path.clone(), modified_time(&job.path))],
        eval_time: Duration::ZERO,
        mesh_time: Duration::ZERO,
    };
    let text = match std::fs::read_to_string(&job.path) {
        Ok(text) => text,
        Err(err) => {
            result.console.push(ConsoleLine {
                kind: LineKind::Error,
                text: format!("Cannot read '{}': {err}", job.path.display()),
            });
            return result;
        }
    };

    let started = Instant::now();
    let mut evaluator = Evaluator::new(EvalOptions {
        cache: Some(cache.clone()),
        ..EvalOptions::with_standard_libraries()
    });
    let (nodes, error) = evaluator.run(&job.path, &text);
    result.eval_time = started.elapsed();

    for file in evaluator.files().iter().skip(1) {
        if file.path.is_file() && !result.dependencies.iter().any(|(p, _)| *p == file.path) {
            result
                .dependencies
                .push((file.path.clone(), modified_time(&file.path)));
        }
    }
    for message in evaluator.messages() {
        push_message(&mut result.console, &evaluator, message);
    }
    if let Some(err) = &error {
        evaluator.take_messages();
        evaluator.report_error(err);
        for message in evaluator.take_messages() {
            push_message(&mut result.console, &evaluator, &message);
        }
        for line in &err.trace {
            result.console.push(ConsoleLine {
                kind: LineKind::Trace,
                text: format!("TRACE: {line}"),
            });
        }
        if err.is_parse_error() {
            return result;
        }
    }

    let started = Instant::now();
    let options = RenderOptions {
        cancel: Some(job.cancel.clone()),
    };
    // import()/surface() paths resolve against the file containing the call.
    let files: Vec<PathBuf> = evaluator.files().iter().map(|f| f.path.clone()).collect();
    match mesher::render_with_files(&nodes, &options, &files) {
        Ok(output) => {
            for diagnostic in &output.diagnostics {
                let location = diagnostic.source.map_or_else(String::new, |s| {
                    let line = evaluator
                        .files()
                        .get(s.file as usize)
                        .map_or(0, |f| f.index.line(s.start.into()));
                    location_suffix(&evaluator, s.file, line)
                });
                let (kind, label) = match diagnostic.level {
                    mesher::Level::Warning => (LineKind::Warning, "WARNING"),
                    mesher::Level::Error => (LineKind::Error, "ERROR"),
                };
                result.console.push(ConsoleLine {
                    kind,
                    text: format!("{label}: {}{location}", diagnostic.text),
                });
            }
            result.output = Some(output);
        }
        Err(mesher::RenderError::Cancelled) => {}
        Err(err) => result.console.push(ConsoleLine {
            kind: LineKind::Error,
            text: format!("ERROR: {err}"),
        }),
    }
    result.mesh_time = started.elapsed();
    result
}

/// ` in file name.scad, line N` (file name only: the console is narrow).
fn location_suffix(evaluator: &Evaluator, file: u32, line: u32) -> String {
    evaluator
        .files()
        .get(file as usize)
        .map(|f| {
            let name = f.path.file_name().map_or_else(
                || f.path.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            format!(" in file {name}, line {line}")
        })
        .unwrap_or_default()
}

fn push_message(console: &mut Vec<ConsoleLine>, evaluator: &Evaluator, message: &Message) {
    let location = match (message.file, message.line) {
        (Some(file), Some(line)) => location_suffix(evaluator, file, line),
        _ => String::new(),
    };
    let (kind, text) = match message.level {
        Level::Echo => (LineKind::Echo, format!("ECHO: {}", message.text)),
        Level::Error => (
            LineKind::Error,
            format!("ERROR: {}{location}", message.text),
        ),
        Level::Trace => (
            LineKind::Trace,
            format!("TRACE: {}{location}", message.text),
        ),
        level => (
            LineKind::Warning,
            format!("{}: {}{location}", level.label(), message.text),
        ),
    };
    console.push(ConsoleLine { kind, text });
}
