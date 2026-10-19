use std::fs;
use std::path::PathBuf;

use fabric_recovery::{
    ApplyOptions, ConflictKind, Operation, RecoveryError, apply_restore, apply_restore_with,
    plan_restore, resume_restore,
};
use fabric_snapshot::{RESTORE_DIR, RelPath, capture};
use fabric_store::{ObjectId, Store, StoreError};
use tempfile::TempDir;

struct Fixture {
    _dir: TempDir,
    project: PathBuf,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        fs::create_dir(&project).unwrap();
        let store = Store::init(dir.path().join("store")).unwrap();
        Self {
            _dir: dir,
            project,
            store,
        }
    }

    fn write(&self, rel: &str, content: &[u8]) {
        let path = self.project.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn read(&self, rel: &str) -> Vec<u8> {
        fs::read(self.project.join(rel)).unwrap()
    }

    fn remove(&self, rel: &str) {
        let path = self.project.join(rel);
        if path.is_dir() {
            fs::remove_dir_all(path).unwrap();
        } else {
            fs::remove_file(path).unwrap();
        }
    }

    fn snapshot(&self) -> ObjectId {
        capture(&self.store, &self.project).unwrap().id
    }

    fn restore(&self, target: &ObjectId) -> fabric_recovery::RestoreReport {
        let plan = plan_restore(&self.store, &self.project, target).unwrap();
        apply_restore(&self.store, &self.project, &plan).unwrap()
    }

    /// The "good" state before an agent runs.
    fn seed(&self) -> ObjectId {
        self.write("README.md", b"# app");
        self.write("src/main.rs", b"fn main() { ok() }");
        self.write("src/util.rs", b"pub fn ok() {}");
        self.write("src/old_name.rs", b"mod moved;");
        self.write("tests/basic.rs", b"#[test] fn t() {}");
        self.snapshot()
    }

    /// What a misbehaving agent does: edits, deletes, renames, creates.
    fn agent_breaks_things(&self) {
        self.write("src/main.rs", b"fn main() { broken( }");
        self.remove("tests");
        fs::rename(
            self.project.join("src/old_name.rs"),
            self.project.join("src/new_name.rs"),
        )
        .unwrap();
        self.write("scratch/attempt1.py", b"print('hmm')");
        self.write("src/extra.rs", b"// unwanted");
    }
}

fn rel(path: &str) -> RelPath {
    RelPath::new(path).unwrap()
}

#[test]
fn restore_undoes_an_agents_changes_and_verifies_the_result() {
    let fx = Fixture::new();
    let good = fx.seed();
    fx.agent_breaks_things();

    let plan = plan_restore(&fx.store, &fx.project, &good).unwrap();
    assert!(plan.conflicts.is_empty(), "{:?}", plan.conflicts);
    assert!(plan.destructive);
    // tests/, tests/basic.rs, src/old_name.rs come back; main.rs is modified;
    // scratch/, scratch/attempt1.py, src/extra.rs, src/new_name.rs go away.
    assert_eq!(
        (
            plan.summary.create,
            plan.summary.modify,
            plan.summary.delete
        ),
        (3, 1, 4)
    );

    let report = apply_restore(&fx.store, &fx.project, &plan).unwrap();
    assert_eq!(report.target, good);
    assert!(report.untracked_kept.is_empty());
    assert_eq!(fx.snapshot(), good);
    assert_eq!(fx.read("src/main.rs"), b"fn main() { ok() }");
    assert!(!fx.project.join("scratch").exists());
    assert!(!fx.project.join(RESTORE_DIR).exists());
}

#[test]
fn every_restore_can_itself_be_undone() {
    let fx = Fixture::new();
    let good = fx.seed();
    fx.agent_breaks_things();
    let broken = fx.snapshot();

    let report = fx.restore(&good);
    assert_eq!(report.undo_snapshot, broken);

    fx.restore(&report.undo_snapshot);
    assert_eq!(fx.snapshot(), broken);
    assert_eq!(fx.read("scratch/attempt1.py"), b"print('hmm')");
}

#[test]
fn restoring_the_current_state_is_a_no_op() {
    let fx = Fixture::new();
    let good = fx.seed();
    let plan = plan_restore(&fx.store, &fx.project, &good).unwrap();
    assert!(plan.operations.is_empty());
    assert!(!plan.destructive);
    apply_restore(&fx.store, &fx.project, &plan).unwrap();
    assert_eq!(fx.snapshot(), good);
}

#[test]
fn handles_files_becoming_directories_and_back() {
    let fx = Fixture::new();
    fx.write("thing", b"a file");
    let as_file = fx.snapshot();
    fx.remove("thing");
    fx.write("thing/inside.txt", b"now a directory");
    let as_dir = fx.snapshot();

    fx.restore(&as_file);
    assert_eq!(fx.read("thing"), b"a file");
    fx.restore(&as_dir);
    assert_eq!(fx.read("thing/inside.txt"), b"now a directory");
}

#[test]
fn ignored_file_in_the_way_is_a_conflict_and_is_never_overwritten() {
    let fx = Fixture::new();
    fx.write("notes.txt", b"tracked version");
    let target = fx.snapshot();
    // Later notes.txt becomes ignored, so the current snapshot cannot see or preserve it.
    fx.write(".gitignore", b"notes.txt\n");
    fx.write("notes.txt", b"precious untracked work");

    let plan = plan_restore(&fx.store, &fx.project, &target).unwrap();
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].path, "notes.txt");
    assert_eq!(
        plan.conflicts[0].kind,
        ConflictKind::UntrackedWouldBeOverwritten
    );

    let err = apply_restore(&fx.store, &fx.project, &plan).unwrap_err();
    assert!(matches!(err, RecoveryError::Conflicts(_)), "{err:?}");
    assert_eq!(fx.read("notes.txt"), b"precious untracked work");
    assert!(fx.project.join(".gitignore").exists());
}

#[test]
fn ignored_file_inside_a_directory_to_remove_is_a_conflict() {
    let fx = Fixture::new();
    fx.write("keep.txt", b"k");
    let target = fx.snapshot();
    fx.write("gen/.gitignore", b"*.bin\n");
    fx.write("gen/model.bin", b"hours of compute");

    let plan = plan_restore(&fx.store, &fx.project, &target).unwrap();
    assert_eq!(plan.conflicts.len(), 1, "{:?}", plan.conflicts);
    assert_eq!(plan.conflicts[0].path, "gen/model.bin");
    assert_eq!(
        plan.conflicts[0].kind,
        ConflictKind::UntrackedInRemovedDirectory
    );
    assert!(apply_restore(&fx.store, &fx.project, &plan).is_err());
    assert_eq!(fx.read("gen/model.bin"), b"hours of compute");
}

#[test]
fn untracked_files_outside_the_plan_are_kept_and_reported() {
    let fx = Fixture::new();
    fx.write("app.txt", b"v1");
    let target = fx.snapshot();
    fx.write(".gitignore", b"*.tmp\n");
    fx.write("cache.tmp", b"ignored data");

    // Removing .gitignore makes cache.tmp visible, but it was never captured and is left
    // alone; verification reports it instead of failing.
    let report = fx.restore(&target);
    assert_eq!(report.untracked_kept, [rel("cache.tmp")]);
    assert_eq!(fx.read("cache.tmp"), b"ignored data");
}

#[test]
fn refuses_a_plan_if_the_project_changed_after_planning() {
    let fx = Fixture::new();
    let good = fx.seed();
    fx.agent_breaks_things();
    let plan = plan_restore(&fx.store, &fx.project, &good).unwrap();

    fx.write("src/main.rs", b"the user kept working");
    let err = apply_restore(&fx.store, &fx.project, &plan).unwrap_err();
    assert!(
        matches!(err, RecoveryError::ProjectChanged { .. }),
        "{err:?}"
    );
    assert_eq!(fx.read("src/main.rs"), b"the user kept working");
    assert!(fx.project.join("scratch/attempt1.py").exists());
}

#[test]
fn corrupt_target_content_fails_before_the_project_is_touched() {
    let fx = Fixture::new();
    let good = fx.seed();
    fx.agent_breaks_things();
    let before = fx.snapshot();

    // Corrupt the stored content of a file the restore must bring back.
    let id = ObjectId::of(b"#[test] fn t() {}");
    let path = fx.store.object_path(&id);
    let mut perms = fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&path, bytes).unwrap();

    let plan = plan_restore(&fx.store, &fx.project, &good).unwrap();
    let err = apply_restore(&fx.store, &fx.project, &plan).unwrap_err();
    assert!(
        matches!(err, RecoveryError::Store(StoreError::Corrupt { .. })),
        "{err:?}"
    );
    assert_eq!(fx.snapshot(), before, "project must be untouched");
    assert!(!fx.project.join(RESTORE_DIR).exists());
}

#[test]
fn missing_target_content_is_refused_at_planning() {
    let fx = Fixture::new();
    let good = fx.seed();
    let path = fx.store.object_path(&ObjectId::of(b"pub fn ok() {}"));
    let mut perms = fs::metadata(&path).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&path, perms).unwrap();
    fs::remove_file(path).unwrap();

    let err = plan_restore(&fx.store, &fx.project, &good).unwrap_err();
    assert!(
        matches!(err, RecoveryError::Store(StoreError::Missing(_))),
        "{err:?}"
    );
}

#[test]
fn a_restore_interrupted_at_any_step_can_be_resumed_to_the_exact_target() {
    let operation_count = {
        let fx = Fixture::new();
        let good = fx.seed();
        fx.agent_breaks_things();
        plan_restore(&fx.store, &fx.project, &good)
            .unwrap()
            .operations
            .len()
    };
    assert!(operation_count > 5);

    for crash_at in 0..operation_count {
        let fx = Fixture::new();
        let good = fx.seed();
        fx.agent_breaks_things();
        let plan = plan_restore(&fx.store, &fx.project, &good).unwrap();
        let options = ApplyOptions {
            simulate_crash_before_op: Some(crash_at),
        };

        let err = apply_restore_with(&fx.store, &fx.project, &plan, &options).unwrap_err();
        assert!(matches!(err, RecoveryError::Interrupted(n) if n == crash_at));

        // A pending restore blocks new ones until it is finished.
        assert!(matches!(
            plan_restore(&fx.store, &fx.project, &good),
            Err(RecoveryError::RestorePending { .. })
        ));

        let report = resume_restore(&fx.store, &fx.project)
            .unwrap()
            .expect("a restore was pending");
        assert_eq!(report.target, good, "crash before op {crash_at}");
        assert_eq!(fx.snapshot(), good, "crash before op {crash_at}");
        assert!(resume_restore(&fx.store, &fx.project).unwrap().is_none());
    }
}

#[test]
fn resume_repeats_an_operation_whose_completion_was_not_recorded() {
    let fx = Fixture::new();
    let good = fx.seed();
    fx.agent_breaks_things();
    let plan = plan_restore(&fx.store, &fx.project, &good).unwrap();
    let crash_at = plan
        .operations
        .iter()
        .position(|op| matches!(op, Operation::WriteFile { .. }))
        .unwrap();
    let options = ApplyOptions {
        simulate_crash_before_op: Some(crash_at + 1),
    };
    apply_restore_with(&fx.store, &fx.project, &plan, &options).unwrap_err();

    // Lose the last progress record, as if the process died between doing the write and
    // recording it.
    let progress = fx.project.join(RESTORE_DIR).join("progress");
    let text = fs::read_to_string(&progress).unwrap();
    let kept: Vec<&str> = text.lines().take(crash_at).collect();
    fs::write(
        &progress,
        kept.join("\n") + if kept.is_empty() { "" } else { "\n" },
    )
    .unwrap();

    resume_restore(&fx.store, &fx.project).unwrap().unwrap();
    assert_eq!(fx.snapshot(), good);
}

/// A snapshot as it might arrive from a Linux machine: names this platform may not hold.
#[cfg(any(windows, target_os = "macos"))]
fn foreign_snapshot(fx: &Fixture, names: &[&str]) -> ObjectId {
    use fabric_snapshot::{Entry, Manifest};
    let id = fx.store.put_bytes(b"content").unwrap().id;
    let entries = names
        .iter()
        .map(|name| Entry::File {
            path: rel(name),
            id,
            size: 7,
            executable: false,
        })
        .collect();
    Manifest::new(entries).unwrap().store(&fx.store).unwrap()
}

#[cfg(windows)]
#[test]
fn windows_reserved_names_are_conflicts() {
    let fx = Fixture::new();
    let target = foreign_snapshot(&fx, &["con.txt", "fine.txt"]);
    let plan = plan_restore(&fx.store, &fx.project, &target).unwrap();
    assert_eq!(plan.conflicts.len(), 1);
    assert_eq!(plan.conflicts[0].path, "con.txt");
    assert!(matches!(
        plan.conflicts[0].kind,
        ConflictKind::UnrepresentableName { .. }
    ));
    assert!(apply_restore(&fx.store, &fx.project, &plan).is_err());
}

#[cfg(any(windows, target_os = "macos"))]
#[test]
fn names_differing_only_by_case_are_conflicts() {
    let fx = Fixture::new();
    let target = foreign_snapshot(&fx, &["README.md", "Readme.md"]);
    let plan = plan_restore(&fx.store, &fx.project, &target).unwrap();
    assert_eq!(plan.conflicts.len(), 1, "{:?}", plan.conflicts);
    assert!(matches!(
        plan.conflicts[0].kind,
        ConflictKind::CaseCollision { .. }
    ));
}

#[test]
fn nothing_pending_means_nothing_to_resume() {
    let fx = Fixture::new();
    fx.seed();
    assert!(resume_restore(&fx.store, &fx.project).unwrap().is_none());
}

#[test]
fn staging_area_from_a_crash_before_the_journal_is_cleaned_up() {
    let fx = Fixture::new();
    let good = fx.seed();
    fx.write(&format!("{RESTORE_DIR}/staged/3.partial"), b"half a file");
    assert!(resume_restore(&fx.store, &fx.project).unwrap().is_none());
    assert!(!fx.project.join(RESTORE_DIR).exists());
    assert_eq!(fx.snapshot(), good);
}
