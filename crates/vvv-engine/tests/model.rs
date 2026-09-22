//! The model kept for a session answers the same questions as one built per
//! call, keeps files and facts between questions, and notices every way the
//! tree can change under it.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Fake;
use vvv_core::Query;
use vvv_engine::{Apply, Engine, Languages, MemoryVfs, Retention, Stamp, Vfs, VfsError, Workspace};

/// A vfs that counts reads, so a test can tell a cache hit from a reload.
#[derive(Debug)]
struct Counting {
    inner: MemoryVfs,
    reads: AtomicUsize,
}

impl Counting {
    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl Vfs for Counting {
    fn read(&self, path: &Path) -> Result<String, VfsError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.inner.read(path)
    }
    fn stamp(&self, path: &Path) -> Result<Stamp, VfsError> {
        self.inner.stamp(path)
    }
    fn write(&self, path: &Path, contents: &str) -> Result<(), VfsError> {
        self.inner.write(path, contents)
    }
    fn exists(&self, path: &Path) -> bool {
        self.inner.exists(path)
    }
    fn rename(&self, from: &Path, to: &Path) -> Result<(), VfsError> {
        self.inner.rename(from, to)
    }
    fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, VfsError> {
        self.inner.walk(root)
    }
}

fn setup() -> (Arc<Counting>, Engine) {
    let vfs = Arc::new(Counting {
        inner: MemoryVfs::new()
            .with_file("/ws/a.p", "def foo\nfoo")
            .with_file("/ws/b.p", "def bar\nbar")
            .with_file("/ws/notes.txt", "foo"),
        reads: AtomicUsize::new(0),
    });
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new().with(Fake::default()),
    )
    .with_retention(Retention::session());
    (vfs, engine)
}

fn paths(engine: &Engine, query: &str) -> Vec<String> {
    let mut out: Vec<String> = engine
        .run(Query::pattern(query))
        .unwrap()
        .matches
        .into_iter()
        .map(|m| m.path.display().to_string())
        .collect();
    out.dedup();
    out
}

#[test]
fn per_call_and_session_agree() {
    let (_, session) = setup();
    let per_call = session.clone().with_retention(Retention::PerCall);
    assert_eq!(paths(&session, "foo"), paths(&per_call, "foo"));
    assert_eq!(paths(&session, "foo"), ["a.p"], "notes.txt has no language");
}

#[test]
fn session_reads_each_file_once_while_unchanged() {
    let (vfs, engine) = setup();
    assert_eq!(paths(&engine, "foo"), ["a.p"]);
    let after_first = vfs.reads();
    assert_eq!(after_first, 2, "both .p files loaded");
    assert_eq!(paths(&engine, "bar"), ["b.p"]);
    assert_eq!(paths(&engine, "foo"), ["a.p"]);
    assert_eq!(vfs.reads(), after_first, "nothing changed, nothing re-read");
}

#[test]
fn session_sees_edits_new_files_deletions_and_renames() {
    let (vfs, engine) = setup();
    assert_eq!(paths(&engine, "foo"), ["a.p"]);

    // Edited: the changed file is the only one re-read.
    vfs.write(Path::new("/ws/b.p"), "def foo\nfoo").unwrap();
    let before = vfs.reads();
    assert_eq!(paths(&engine, "foo"), ["a.p", "b.p"]);
    assert_eq!(vfs.reads(), before + 1);

    // Appeared.
    vfs.write(Path::new("/ws/c.p"), "foo").unwrap();
    assert_eq!(paths(&engine, "foo"), ["a.p", "b.p", "c.p"]);

    // Renamed: the old path is gone, the new one is loaded.
    vfs.rename(Path::new("/ws/c.p"), Path::new("/ws/d.p"))
        .unwrap();
    assert_eq!(paths(&engine, "foo"), ["a.p", "b.p", "d.p"]);

    // Vanished (a rename to a path no language claims).
    vfs.rename(Path::new("/ws/d.p"), Path::new("/ws/d.txt"))
        .unwrap();
    assert_eq!(paths(&engine, "foo"), ["a.p", "b.p"]);
}

#[test]
fn session_sees_what_the_engine_itself_applies() {
    let (_, engine) = setup();
    assert_eq!(paths(&engine, "foo"), ["a.p"]);
    let intent = vvv_engine::RenameIntent::new("foo", "qux");
    let rename = engine.run(intent.clone()).unwrap();
    engine.run(Apply(rename)).unwrap();
    assert_eq!(paths(&engine, "foo"), Vec::<String>::new());
    assert_eq!(paths(&engine, "qux"), ["a.p"]);
}

/// A session that trusts its walk does not look at the tree again within
/// the window — a burst of questions costs one walk — but the engine's own
/// writes, and being told, end the trust at once.
#[test]
fn a_trusting_session_walks_once_per_window_unless_touched() {
    let (vfs, session) = setup();
    let engine =
        session.with_retention(Retention::session().trusting(std::time::Duration::from_secs(60)));
    assert_eq!(paths(&engine, "foo"), ["a.p"]);
    let after_first = vfs.reads();

    vfs.write(Path::new("/ws/b.p"), "def foo\nfoo").unwrap();
    assert_eq!(
        paths(&engine, "foo"),
        ["a.p"],
        "within the window: not looked at"
    );
    assert_eq!(vfs.reads(), after_first);

    engine.touched();
    assert_eq!(paths(&engine, "foo"), ["a.p", "b.p"], "told: looked at");

    // Its own apply is a change it knows about.
    let rename = engine
        .run(vvv_engine::RenameIntent::new("foo", "qux").declared_in("a.p"))
        .unwrap();
    engine.run(Apply(rename)).unwrap();
    assert_eq!(paths(&engine, "qux"), ["a.p"]);
}

/// A rename asks a file for its references and its imports; with facts on the
/// candidate that is one parse, and with a session model it stays one parse
/// across renames until the file changes.
#[test]
fn a_file_is_parsed_once_per_stamp() {
    use std::sync::atomic::AtomicUsize;

    let parses = Arc::new(AtomicUsize::new(0));
    let vfs = Arc::new(
        MemoryVfs::new()
            .with_file("/ws/a.p", "def foo\nfoo")
            .with_file("/ws/b.p", "use a.p/*\nfoo foo"),
    );
    let engine = Engine::new(
        Workspace::new("/ws", vfs.clone()),
        Languages::new().with(common::Counting::new(parses.clone())),
    )
    .with_retention(Retention::session());
    let intent = vvv_engine::RenameIntent::new("foo", "bar");
    engine.run(intent.clone()).unwrap();
    let first = parses.load(Ordering::SeqCst);
    assert_eq!(first, 2, "each of the two files parsed exactly once");
    engine.run(intent.clone()).unwrap();
    assert_eq!(
        parses.load(Ordering::SeqCst),
        first,
        "nothing changed, nothing re-parsed"
    );
    vfs.write(Path::new("/ws/b.p"), "use a.p/*\nfoo").unwrap();
    engine.run(intent.clone()).unwrap();
    assert_eq!(
        parses.load(Ordering::SeqCst),
        first + 1,
        "only the changed file"
    );
}
