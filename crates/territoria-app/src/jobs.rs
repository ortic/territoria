//! Background work: imports, mesh building, loading and saving. Jobs run on
//! their own threads and report back through a channel; the UI thread
//! never waits for them.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};

use territoria_data::Table;
use territoria_io::{ImportedLayer, TableInfo, TableOptions};
use territoria_model::{LayerGeometry, LayerId, Project};
use territoria_render::LayerMesh;

/// A boundary layer read and prepared (topology, neighbours) off the UI thread.
pub struct PreparedLayer {
    pub imported: ImportedLayer,
    pub geometry: LayerGeometry,
}

pub enum JobResult {
    Boundaries {
        path: PathBuf,
        result: Result<Vec<PreparedLayer>, String>,
    },
    Table {
        path: PathBuf,
        options: TableOptions,
        result: Result<(Table, TableInfo), String>,
    },
    Mesh {
        layer: LayerId,
        crs: String,
        kind: crate::meshes::MeshKind,
        rev: u64,
        mesh: Arc<LayerMesh>,
    },
    Loaded {
        path: PathBuf,
        result: Result<Project, String>,
    },
    Saved {
        path: PathBuf,
        result: Result<(), String>,
    },
    Catalog(Result<Vec<territoria_io::online::BoundarySet>, String>),
    Downloaded {
        set: territoria_io::online::BoundarySet,
        result: Result<PathBuf, String>,
    },
    Exported {
        path: PathBuf,
        result: Result<(), String>,
    },
    Optimized {
        plan: territoria_model::PlanId,
        result: Result<crate::territories::Proposal, String>,
    },
    /// A newer version of a layer's boundaries, read.
    Replacement {
        layer: LayerId,
        path: PathBuf,
        result: Result<Vec<PreparedLayer>, String>,
    },
    /// The map as an image for the clipboard.
    Copied(Result<egui::ColorImage, String>),
    /// Addresses of a table to be added, found (or not).
    Geocoded {
        name: String,
        table: Table,
        path: PathBuf,
        result: Result<territoria_io::geocode::Geocoded, String>,
    },
}

pub struct Running {
    pub id: u64,
    pub label: String,
    /// Can be stopped from the status bar.
    pub cancellable: bool,
    cancel: Arc<AtomicBool>,
    /// Share done in thousandths, if the job reports it.
    progress: Arc<AtomicU32>,
}

impl Running {
    pub fn progress(&self) -> Option<f32> {
        match self.progress.load(Ordering::Relaxed) {
            u32::MAX => None,
            p => Some(p as f32 / 1000.0),
        }
    }
}

/// What a running job sees: whether it should stop, and where to report
/// progress.
pub struct Job {
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU32>,
    ctx: Option<egui::Context>,
}

impl Job {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Reports the share done (0…1).
    pub fn progress(&self, share: f32) {
        let p = (share.clamp(0.0, 1.0) * 1000.0) as u32;
        if self.progress.swap(p, Ordering::Relaxed) != p
            && let Some(ctx) = &self.ctx
        {
            ctx.request_repaint();
        }
    }
}

pub struct Jobs {
    tx: Sender<(u64, JobResult)>,
    rx: Receiver<(u64, JobResult)>,
    pub running: Vec<Running>,
    next: u64,
    ctx: Option<egui::Context>,
}

impl Default for Jobs {
    fn default() -> Self {
        let (tx, rx) = channel();
        Jobs {
            tx,
            rx,
            running: Vec::new(),
            next: 0,
            ctx: None,
        }
    }
}

impl Jobs {
    pub fn set_context(&mut self, ctx: egui::Context) {
        self.ctx = Some(ctx);
    }

    /// Runs `work` on a new thread; `label` shows in the status bar (empty
    /// for quiet jobs).
    pub fn spawn(
        &mut self,
        label: impl Into<String>,
        work: impl FnOnce(&Job) -> JobResult + Send + 'static,
    ) -> u64 {
        self.spawn_with(label, false, work)
    }

    /// Like [`Jobs::spawn`], with a Stop button in the status bar.
    pub fn spawn_cancellable(
        &mut self,
        label: impl Into<String>,
        work: impl FnOnce(&Job) -> JobResult + Send + 'static,
    ) -> u64 {
        self.spawn_with(label, true, work)
    }

    fn spawn_with(
        &mut self,
        label: impl Into<String>,
        cancellable: bool,
        work: impl FnOnce(&Job) -> JobResult + Send + 'static,
    ) -> u64 {
        self.next += 1;
        let id = self.next;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(AtomicU32::new(u32::MAX));
        let job = Job {
            cancel: cancel.clone(),
            progress: progress.clone(),
            ctx: self.ctx.clone(),
        };
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::Builder::new()
            .name("territoria-job".into())
            .spawn(move || {
                let result = work(&job);
                let _ = tx.send((id, result));
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            })
            .expect("spawning a worker thread");
        self.running.push(Running {
            id,
            label: label.into(),
            cancellable,
            cancel,
            progress,
        });
        id
    }

    pub fn poll(&mut self) -> Vec<JobResult> {
        let mut out = Vec::new();
        while let Ok((id, result)) = self.rx.try_recv() {
            self.running.retain(|r| r.id != id);
            out.push(result);
        }
        out
    }

    pub fn cancel(&self, id: u64) {
        if let Some(r) = self.running.iter().find(|r| r.id == id) {
            r.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Labels of jobs worth showing.
    pub fn visible(&self) -> impl Iterator<Item = &Running> {
        self.running.iter().filter(|r| !r.label.is_empty())
    }

    pub fn is_busy(&self) -> bool {
        !self.running.is_empty()
    }
}
