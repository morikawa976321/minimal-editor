//! All memo filesystem work is serialized on one thread. The window thread
//! keeps its own presentation snapshot and never waits for this worker.
use crate::store::{Memo, Store};
use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Debug)]
pub enum Action {
    Create,
    Switch { id: String, restoring: bool },
    Archive { id: String },
    Restore { id: String },
    Rename { id: String, title: String },
}

#[derive(Clone, Debug)]
pub enum Request {
    Save {
        id: String,
        text: String,
        generation: u64,
    },
    Action(Action),
}

#[derive(Clone, Debug)]
pub struct View {
    pub normal: Vec<Memo>,
    pub archived: Vec<Memo>,
    pub active: Option<String>,
}

impl View {
    pub fn read(store: &Store) -> Self {
        Self {
            normal: store.list(false),
            archived: store.list(true),
            active: store.active(),
        }
    }
}

pub struct Reply {
    pub request: Request,
    pub view: View,
    pub document: Option<(Option<String>, String)>,
    pub error: Option<String>,
    pub recovery: Option<PathBuf>,
    pub save_ms: f64,
}

pub struct Worker {
    requests: Sender<Request>,
    replies: Receiver<Reply>,
}

impl Worker {
    pub fn start(mut store: Store, delay: Duration) -> Result<Self, String> {
        let (requests, incoming) = mpsc::channel::<Request>();
        let (outgoing, replies) = mpsc::channel::<Reply>();
        thread::Builder::new()
            .name("memo-storage".into())
            .spawn(move || {
                // Repeated retries of the same unsaved buffer reuse its recovery
                // copy. A successful save resets this exactly as in the UI store.
                let mut recovered: Option<(u64, PathBuf)> = None;
                while let Ok(request) = incoming.recv() {
                    let mut document = None;
                    let mut recovery = None;
                    let mut save_ms = 0.0;
                    let result = match &request {
                        Request::Save { id, text, .. } => {
                            let started = Instant::now();
                            if !delay.is_zero() {
                                thread::sleep(delay);
                            }
                            let result = match store.save(id, text) {
                                Ok(()) => {
                                    recovered = None;
                                    Ok(())
                                }
                                Err(error) => {
                                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                                    id.hash(&mut hash);
                                    text.hash(&mut hash);
                                    let fingerprint = hash.finish();
                                    if let Some((_, path)) = recovered.as_ref().filter(|(old, _)| *old == fingerprint) {
                                        recovery = Some(path.clone());
                                        Err(error)
                                    } else {
                                        match store.recover(id, text) {
                                            Ok(path) => {
                                                recovered = Some((fingerprint, path.clone()));
                                                recovery = Some(path);
                                                Err(error)
                                            }
                                            Err(recovery_error) => Err(format!(
                                                "{error}\n復元用コピーも保存できません: {recovery_error}"
                                            )),
                                        }
                                    }
                                }
                            };
                            save_ms = started.elapsed().as_secs_f64() * 1000.0;
                            result
                        }
                        Request::Action(action) => apply_action(&mut store, action).map(|value| {
                            document = value;
                        }),
                    };
                    let reply = Reply {
                        request,
                        view: View::read(&store),
                        document,
                        error: result.err(),
                        recovery,
                        save_ms,
                    };
                    if outgoing.send(reply).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| format!("保存スレッドを起動できません: {e}"))?;
        Ok(Self { requests, replies })
    }

    pub fn send(&self, request: Request) -> Result<(), String> {
        self.requests
            .send(request)
            .map_err(|_| "保存スレッドが停止しました。本文は画面に残しています".into())
    }

    pub fn poll(&self) -> Result<Option<Reply>, String> {
        match self.replies.try_recv() {
            Ok(reply) => Ok(Some(reply)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err("保存スレッドが停止しました。本文は画面に残しています".into())
            }
        }
    }
}

fn apply_action(
    store: &mut Store,
    action: &Action,
) -> Result<Option<(Option<String>, String)>, String> {
    match action {
        Action::Create => {
            let id = store.create()?;
            let text = store.text(&id)?;
            Ok(Some((Some(id), text)))
        }
        Action::Switch { id, restoring } => {
            // Validate and obtain the complete target before publishing its
            // activation. The UI must not discard its old buffer on an error.
            let text = store.text(id)?;
            if *restoring {
                store.archive(id, false)?;
            }
            store.activate(id)?;
            Ok(Some((Some(id.clone()), text)))
        }
        Action::Archive { id } => {
            let before = store.active();
            let next = if before.as_deref() == Some(id.as_str()) {
                store
                    .list(false)
                    .into_iter()
                    .find(|m| m.id != *id)
                    .map(|m| m.id)
            } else {
                before.clone()
            };
            let text = if before != next {
                Some(match next.as_deref() {
                    Some(id) => store.text(id)?,
                    None => String::new(),
                })
            } else {
                None
            };
            store.archive(id, true)?;
            Ok(text.map(|text| (store.active(), text)))
        }
        Action::Restore { id } => {
            let was_empty = store.active().is_none();
            let text = if was_empty {
                Some(store.text(id)?)
            } else {
                None
            };
            store.archive(id, false)?;
            Ok(text.map(|text| (store.active(), text)))
        }
        Action::Rename { id, title } => {
            store.rename(id, title)?;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "minimal-native-worker-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            // The detached worker releases the lock after its sender is dropped.
            for _ in 0..100 {
                if fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                    return;
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
    }
    fn reply(worker: &Worker) -> Reply {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(reply) = worker.poll().unwrap() {
                return reply;
            }
            assert!(Instant::now() < deadline, "Storage reply timed out");
            thread::sleep(Duration::from_millis(2));
        }
    }
    fn action(worker: &Worker, action: Action) -> Reply {
        worker.send(Request::Action(action)).unwrap();
        let result = reply(worker);
        assert!(result.error.is_none(), "{:?}", result.error);
        result
    }

    #[test]
    fn delayed_save_returns_without_waiting_and_keeps_generation() {
        let temp = Temp::new();
        let store = Store::open(temp.0.clone()).unwrap();
        let id = store.active().unwrap();
        let delay = Duration::from_millis(150);
        let worker = Worker::start(store, delay).unwrap();
        worker
            .send(Request::Save {
                id: id.clone(),
                text: "確定した本文".into(),
                generation: 42,
            })
            .unwrap();
        assert!(worker.poll().unwrap().is_none());
        let result = reply(&worker);
        assert!(matches!(
            result.request,
            Request::Save { generation: 42, .. }
        ));
        assert!(result.error.is_none());
        assert!(result.save_ms >= delay.as_secs_f64() * 1000.0);
        assert!(result.document.is_none());
        assert_eq!(
            fs::read_to_string(temp.0.join("memos").join(format!("{id}.txt"))).unwrap(),
            "確定した本文"
        );
    }

    #[test]
    fn actions_return_only_documents_that_change_the_editor() {
        let temp = Temp::new();
        let store = Store::open(temp.0.clone()).unwrap();
        let first = store.active().unwrap();
        let worker = Worker::start(store, Duration::ZERO).unwrap();
        let created = action(&worker, Action::Create);
        let second = created.view.active.unwrap();
        assert_eq!(
            created.document,
            Some((Some(second.clone()), String::new()))
        );
        let renamed = action(
            &worker,
            Action::Rename {
                id: second.clone(),
                title: "名前".into(),
            },
        );
        assert!(renamed.document.is_none());
        assert!(renamed
            .view
            .normal
            .iter()
            .any(|m| m.id == second && m.title == "名前"));
        assert!(action(&worker, Action::Archive { id: first.clone() })
            .document
            .is_none());
        let all_archived = action(&worker, Action::Archive { id: second.clone() });
        assert_eq!(all_archived.document, Some((None, String::new())));
        assert_eq!(all_archived.view.archived.len(), 2);
        let restored = action(&worker, Action::Restore { id: first.clone() });
        assert_eq!(
            restored.document,
            Some((Some(first.clone()), String::new()))
        );
        let switched = action(
            &worker,
            Action::Switch {
                id: second.clone(),
                restoring: true,
            },
        );
        assert_eq!(switched.document, Some((Some(second), String::new())));
        assert_eq!(switched.view.normal.len(), 2);
    }

    #[test]
    fn conflict_keeps_external_file_and_reuses_identical_recovery() {
        let temp = Temp::new();
        let store = Store::open(temp.0.clone()).unwrap();
        let id = store.active().unwrap();
        let path = temp.0.join("memos").join(format!("{id}.txt"));
        let worker = Worker::start(store, Duration::ZERO).unwrap();
        fs::write(&path, "外部の本文").unwrap();
        let mut recovery = None;
        for generation in 1..=2 {
            worker
                .send(Request::Save {
                    id: id.clone(),
                    text: "未保存本文".into(),
                    generation,
                })
                .unwrap();
            let result = reply(&worker);
            assert!(result.error.is_some());
            let copy = result.recovery.unwrap();
            assert_eq!(fs::read_to_string(&copy).unwrap(), "未保存本文");
            if let Some(old) = recovery {
                assert_eq!(old, copy);
            }
            recovery = Some(copy);
        }
        worker
            .send(Request::Save {
                id,
                text: "追加された未保存本文".into(),
                generation: 3,
            })
            .unwrap();
        let changed = reply(&worker);
        assert!(changed.error.is_some());
        assert_ne!(changed.recovery, recovery);
        assert_eq!(fs::read_to_string(path).unwrap(), "外部の本文");
    }
}
