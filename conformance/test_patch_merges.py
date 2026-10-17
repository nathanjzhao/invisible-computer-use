"""Every merge that touched cua-driver.patch kept what both parents carried.

A textual merge of the patch file can silently drop one side's hunks (e737b7b
lost two Linux fixes that way), and `git merge -s ours` drops a side's patch
change without any conflict. This walks every merge in the history and runs
impl/native/merge-patch.sh --check-merge on each one where a parent's patch
differs from the base or from the result. The only merges skipped are those
where one parent is the base and the other is the result: the base side
contributes nothing, so nothing can be lost.
"""
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
PATCH = "impl/native/cua-driver.patch"
# Known lossy merges, each paired with the commit that restored what it lost.
KNOWN_LOSSY = {"e737b7b": "f1c40b6"}


def git(*args):
    return subprocess.run(["git", "-C", str(ROOT), *args], check=True,
                          capture_output=True, text=True).stdout.strip()


def blob(rev):
    try:
        return git("rev-parse", f"{rev}:{PATCH}")
    except subprocess.CalledProcessError:
        return None


def audited(b, m, p1, p2):
    """Whether a merge with these patch blobs (base, result, parents) needs --check-merge."""
    if b == m == p1 == p2:
        return False
    return not ({p1, p2} == {b, m} and b != m)


def risky_merges():
    # Every merge, not `git log -- PATCH`: history simplification hides a merge
    # whose result equals one parent, which is exactly what `-s ours` produces.
    out = []
    for sha in git("rev-list", "--merges", "HEAD").split():
        p1, p2 = git("rev-parse", f"{sha}^1"), git("rev-parse", f"{sha}^2")
        bases = git("merge-base", "--all", p1, p2).split()
        if any(audited(blob(b), blob(sha), blob(p1), blob(p2)) for b in bases):
            out.append(sha[:7])
    return out


MERGES = risky_merges()


def test_the_filter_audits_every_merge_that_could_lose_a_side():
    # b: base, m: result, x/y: changed patches. An `-s ours` merge of a branch
    # that changed the patch keeps the base (C5 in the round-2 review).
    assert audited("b", "b", "b", "y")        # -s ours dropped theirs
    assert audited("b", "b", "x", "b")        # -s theirs dropped ours
    assert audited("b", "m", "x", "y")        # both changed
    assert audited("b", "x", "x", "y")        # one side's change dropped
    assert audited("b", "m", "b", "b")        # patch edited in the merge itself
    assert not audited("b", "x", "x", "b")    # only ours changed and it is the result
    assert not audited("b", "y", "b", "y")
    assert not audited("b", "b", "b", "b")


def test_history_has_the_known_lossy_merge():
    # Guards the walk itself: if it stopped finding e737b7b, the test below
    # would pass vacuously.
    assert set(KNOWN_LOSSY) <= set(MERGES), MERGES


@pytest.mark.parametrize("sha", MERGES)
def test_patch_merge_keeps_both_sides(sha):
    r = subprocess.run([str(ROOT / "impl/native/merge-patch.sh"), "--check-merge", sha],
                       capture_output=True, text=True)
    if sha in KNOWN_LOSSY:
        assert r.returncode != 0, f"{sha} is known lossy but the check passed it"
        fix = KNOWN_LOSSY[sha]
        assert git("merge-base", "--is-ancestor", fix, "HEAD") == "", f"restoring commit {fix} missing"
        return
    assert r.returncode == 0, r.stdout + r.stderr


# f64d4a5 merged fix/windows-review2, whose own merge 3a4b090 made the history
# criss-cross: git's first merge base (8b5d5df) predates changes both sides share,
# so --check-merge must try every base.
CRISS_CROSS = "f64d4a5"


def test_a_criss_cross_merge_is_checked_against_every_base():
    p1, p2 = git("rev-parse", f"{CRISS_CROSS}^1"), git("rev-parse", f"{CRISS_CROSS}^2")
    assert len(git("merge-base", "--all", p1, p2).split()) > 1
    assert CRISS_CROSS in MERGES
    # The same parents with ours' tree (an `-s ours` merge) drops the Windows fixes
    # and must fail against every base, not only git's first pick.
    lossy = git("commit-tree", f"{p1}^{{tree}}", "-p", p1, "-p", p2, "-m", "synthetic -s ours")
    r = subprocess.run([str(ROOT / "impl/native/merge-patch.sh"), "--check-merge", lossy],
                       capture_output=True, text=True)
    assert r.returncode != 0 and "lossy against every merge base" in r.stderr, r.stdout + r.stderr
    assert "platform-windows" in r.stdout + r.stderr


# ---------------------------------------------------------------------------
# merge-patch.sh itself, on a synthetic vendor repo (round 3, app-r3 A3, A5, H7).
# A throwaway vendor with one pinned commit and a throwaway repo holding a copy
# of merge-patch.sh; each "commit" there is one version of cua-driver.patch.

VPATH = "libs/cua-driver/rust/crates/x"
MAN = "libs/cua-driver/contract/manifest.json"
OUTSIDE = "libs/cua-driver/scripts/s.sh"


def run(*args, cwd, env=None, check=True):
    r = subprocess.run(list(args), cwd=cwd, env=env, capture_output=True, text=True)
    if check and r.returncode:
        raise AssertionError(f"{args}: {r.stdout}{r.stderr}")
    return r


def gitc(*args, cwd):
    return run("git", "-c", "user.email=t@t", "-c", "user.name=t", *args, cwd=cwd).stdout.strip()


class Synth:
    def __init__(self, root):
        import os, shutil
        self.root = root
        self.vendor, self.fake, self.tree = root / "vendor", root / "fake", root / "t"
        (self.vendor / VPATH).mkdir(parents=True)
        gitc("init", "-q", "-b", "main", cwd=self.vendor)
        for f in ("a", "b"):
            (self.vendor / VPATH / f"{f}.rs").write_text("".join(f"{f} line {i}\n" for i in range(1, 41)))
        # a generated manifest the patch also carries, and a vendor file the patch must not touch
        (self.vendor / MAN).parent.mkdir(parents=True)
        (self.vendor / MAN).write_text("".join(f"manifest line {i}\n" for i in range(1, 41)))
        (self.vendor / OUTSIDE).parent.mkdir(parents=True)
        (self.vendor / OUTSIDE).write_text("".join(f"script line {i}\n" for i in range(1, 11)))
        gitc("add", "-A", cwd=self.vendor); gitc("commit", "-qm", "pin", cwd=self.vendor)
        self.pin = gitc("rev-parse", "HEAD", cwd=self.vendor)
        gitc("worktree", "add", "-q", "--detach", str(self.tree), self.pin, cwd=self.vendor)
        (self.fake / "impl/native").mkdir(parents=True)
        gitc("init", "-q", "-b", "main", cwd=self.fake)
        shutil.copy(ROOT / "impl/native/merge-patch.sh", self.fake / "impl/native/merge-patch.sh")
        shutil.copy(ROOT / "impl/native/patch-paths.txt", self.fake / "impl/native/patch-paths.txt")
        (self.fake / "vendor-cua").symlink_to(self.vendor)
        (self.fake / "README").write_text("x\n")
        gitc("add", "-A", cwd=self.fake); gitc("commit", "-qm", "init", cwd=self.fake)
        self.env = {**os.environ, "CUA_VENDOR_PIN": self.pin}
        self.base = self.variant("base", "main")

    def edit(self, f, n, suffix="", insert=None, delete=False):
        p = self.tree / VPATH / f
        lines = p.read_text().splitlines(True)
        if insert is not None:
            lines.insert(n, insert + "\n")
        elif delete:
            del lines[n - 1]
        else:
            lines[n - 1] = lines[n - 1].rstrip("\n") + suffix + "\n"
        p.write_text("".join(lines))

    def edit_file(self, path, n, suffix="", insert=None):
        p = self.tree / path
        lines = p.read_text().splitlines(True)
        if insert is not None:
            lines.insert(n, insert + "\n")
        else:
            lines[n - 1] = lines[n - 1].rstrip("\n") + suffix + "\n"
        p.write_text("".join(lines))

    def edit_manifest(self, n, suffix="", insert=None):
        self.edit_file(MAN, n, suffix, insert)

    def base_patch(self):
        self.edit("a.rs", 20, insert="FIX")          # base patch adds FIX as line 21
        self.edit_manifest(20, insert="MANIFEST-FIX")  # and a manifest entry as line 21 of the manifest

    def reset(self):
        gitc("checkout", "-q", "-f", "--detach", self.pin, cwd=self.tree)
        gitc("clean", "-qfd", cwd=self.tree)

    def snap(self, name):
        gitc("add", "-A", cwd=self.tree)
        patch = run("git", "diff", "--cached", "--binary", self.pin, "--", "libs/cua-driver", cwd=self.tree).stdout
        (self.fake / "impl/native/cua-driver.patch").write_text(patch)
        gitc("add", "-A", cwd=self.fake); gitc("commit", "-qm", name, cwd=self.fake)
        self.reset()
        return gitc("rev-parse", "--short", "HEAD", cwd=self.fake)

    def variant(self, name, start, *edits):
        gitc("checkout", "-q", "-B", name, start, cwd=self.fake)
        self.reset(); self.base_patch()
        for e in edits:
            e()
        return self.snap(name)

    def merge_commit(self, name, ours, theirs, *edits):
        """A merge of ours and theirs whose patch is the base patch plus `edits`."""
        gitc("checkout", "-q", "-B", name, ours, cwd=self.fake)
        run("git", "-c", "user.email=t@t", "-c", "user.name=t", "merge", "-q", "--no-commit", "-s", "ours", theirs, cwd=self.fake, check=False)
        self.reset(); self.base_patch()
        for e in edits:
            e()
        return self.snap(name)

    def mp(self, *args, cwd=None):
        return run("bash", str(self.fake / "impl/native/merge-patch.sh"), *args, cwd=cwd or self.fake, env=self.env, check=False)


@pytest.fixture(scope="module")
def synth(tmp_path_factory):
    return Synth(tmp_path_factory.mktemp("merge-patch"))


def test_a_dropped_base_patch_line_two_lines_from_a_side_edit_is_refused(synth):
    # app-r3 A3/H1: with the old 2-line slack this lossy merge was accepted.
    s = synth
    ours = lambda n: lambda: s.edit("a.rs", n, " ours")
    theirs = lambda: s.edit("a.rs", 5, " theirs")
    drop_fix = lambda: s.edit("a.rs", 21, delete=True)
    o2, o3 = s.variant("o2", s.base, ours(23)), s.variant("o3", s.base, ours(24))
    th = s.variant("th", s.base, theirs)
    for side, n in ((o2, 23), (o3, 24)):
        r = s.mp("--check-merge", s.merge_commit(f"lossy-{n}", side, th, ours(n), theirs, drop_fix))
        assert r.returncode != 0 and "dropped base-patch changes" in r.stderr, (n, r.stdout + r.stderr)
    r = s.mp("--check-merge", s.merge_commit("good", o2, th, ours(23), theirs))
    assert r.returncode == 0, r.stdout + r.stderr


def test_a_relative_output_path_is_written_where_the_script_was_run(synth, tmp_path):
    # app-r3 H7: "wrote rel.patch" landed in the temp clone and was deleted with it.
    s = synth
    o = s.variant("rel-o", s.base, lambda: s.edit("a.rs", 30, " ours"))
    t = s.variant("rel-t", s.base, lambda: s.edit("a.rs", 5, " theirs"))
    (tmp_path / "out").mkdir()
    r = s.mp(s.base, o, t, "out/rel.patch", cwd=tmp_path)
    assert r.returncode == 0, r.stdout + r.stderr
    written = tmp_path / "out/rel.patch"
    assert written.is_file(), r.stdout
    assert f"wrote {written}" in r.stdout, r.stdout


# ---------------------------------------------------------------------------
# The generated contract and bindings are carried by the patch (gaps-linux-2, item 4). Before, the
# pathspec was the crates and the lockfile only, so contract/manifest.json and the Python bindings
# regenerated from a patched source stayed in the vendor working tree, and a change to a native
# argument was in the Rust patch but not in the checked-in manifest.

def patch_paths():
    return [l.strip() for l in (ROOT / "impl/native/patch-paths.txt").read_text().splitlines() if l.strip() and not l.startswith("#")]


def test_one_list_of_patched_paths_is_read_by_both_tools_and_names_the_generated_files():
    paths = patch_paths()
    assert "libs/cua-driver/rust/crates" in paths and "libs/cua-driver/rust/Cargo.lock" in paths
    assert MAN in paths, "the checked-in tool contract is part of the patch"
    assert "libs/cua-driver/python/src/cua_driver/_native.py" in paths and "libs/cua-driver/python/src/cua_driver/_native_contract.py" in paths
    for script in ("merge-patch.sh", "apply-and-build.sh"):
        text = (ROOT / "impl/native" / script).read_text()
        assert "patch-paths.txt" in text, f"{script} does not read the shared list"
        assert "libs/cua-driver/rust/crates" not in text, f"{script} still carries its own copy of the pathspec"


def patch_sections():
    import re
    text = (ROOT / PATCH).read_text()
    return {m.group(1): sec for sec in re.split(r"^(?=diff --git )", text, flags=re.M) if sec
            for m in [re.match(r"diff --git a/(\S+) b/", sec)] if m}


def added_lines(section):
    return "\n".join(l[1:] for l in section.splitlines() if l.startswith("+") and not l.startswith("+++"))


# The arguments the native patch added to the typed contract; the manifest must list them.
CONTRACT_ARGS = ("close_enough_distance_px", "close_enough_progress")
MANIFEST_ENTRIES = 3


@pytest.mark.parametrize("arg", CONTRACT_ARGS)
def test_the_tracked_patch_lists_each_new_native_contract_argument_in_the_manifest_and_the_python_bindings(arg):
    sections = patch_sections()
    assert MAN in sections, "the patch does not carry contract/manifest.json"
    # three typed places carry it (the motion input, its output and the set_agent_cursor_motion schema); a dropped
    # entry leaves the others, so the count is what fails
    entries = added_lines(sections[MAN]).count(f'"{arg}": {{')
    assert entries == MANIFEST_ENTRIES, f"the manifest the patch adds lists {arg} {entries} times, not {MANIFEST_ENTRIES}"
    py = "libs/cua-driver/python/src/cua_driver/_native_contract.py"
    assert py in sections and arg in added_lines(sections[py]), f"{arg} is missing from the Python bindings the patch carries"
    # and it is a real argument of the source the patch also carries
    assert any(arg in added_lines(sec) for name, sec in sections.items() if name.startswith("libs/cua-driver/rust/crates/cua-driver-contract/"))


def test_a_side_that_changes_the_manifest_is_carried_into_the_merge(synth, tmp_path):
    s = synth
    o = s.variant("man-o", s.base, lambda: s.edit_manifest(36, " ours"))
    t = s.variant("man-t", s.base, lambda: s.edit("a.rs", 5, " theirs"))
    out = tmp_path / "man.patch"
    r = s.mp(s.base, o, t, str(out))
    assert r.returncode == 0, r.stdout + r.stderr
    patch = out.read_text()
    assert f"a/{MAN}" in patch and "manifest line 35 ours" in patch, "ours' manifest change is in the written patch"
    assert "a line 5 theirs" in patch and "MANIFEST-FIX" in patch, "theirs' change and the base patch's manifest entry too"


def test_a_merge_that_drops_a_manifest_entry_is_refused(synth):
    s = synth
    o = s.variant("md-o", s.base, lambda: s.edit_manifest(36, " ours"))
    t = s.variant("md-t", s.base, lambda: s.edit("a.rs", 5, " theirs"))
    drop_ours = s.merge_commit("md-drop-ours", o, t, lambda: s.edit("a.rs", 5, " theirs"))
    r = s.mp("--check-merge", drop_ours)
    assert r.returncode != 0 and "manifest.json" in r.stdout + r.stderr, r.stdout + r.stderr
    drop_base = s.merge_commit("md-drop-base", o, t, lambda: s.edit_manifest(36, " ours"), lambda: s.edit("a.rs", 5, " theirs"),
                               lambda: (s.tree / MAN).write_text((s.tree / MAN).read_text().replace("MANIFEST-FIX\n", "")))
    r = s.mp("--check-merge", drop_base)
    assert r.returncode != 0 and "dropped base-patch changes" in r.stderr and "manifest.json" in r.stderr, r.stdout + r.stderr
    good = s.merge_commit("md-good", o, t, lambda: s.edit_manifest(36, " ours"), lambda: s.edit("a.rs", 5, " theirs"))
    r = s.mp("--check-merge", good)
    assert r.returncode == 0, r.stdout + r.stderr


def test_a_vendor_file_outside_the_listed_paths_is_still_refused(synth):
    s = synth
    o = s.variant("out-o", s.base, lambda: s.edit_file(OUTSIDE, 3, " ours"))
    t = s.variant("out-t", s.base, lambda: s.edit("a.rs", 5, " theirs"))
    r = s.mp(s.base, o, t)
    assert r.returncode != 0 and "outside the patched paths" in r.stderr and "scripts/s.sh" in r.stderr, r.stdout + r.stderr


def resume_case(s, resolve, extra_commit=None):
    """Start a conflicting merge, resolve a.rs with `resolve(text) -> text`, commit, resume."""
    c1 = s.variant("c1", s.base, lambda: s.edit("a.rs", 10, " (ours)"))
    c2 = s.variant("c2", s.base, lambda: s.edit("a.rs", 10, " (theirs)"))
    out = s.root / "c.patch"
    out.unlink(missing_ok=True)
    r = s.mp(s.base, c1, c2, str(out))
    assert r.returncode != 0 and "--resume" in r.stderr, r.stdout + r.stderr
    work = r.stderr.split("--resume ", 1)[1].split()[0]
    v = Path(work) / "v"
    f = v / VPATH / "a.rs"
    f.write_text(resolve(f.read_text()))
    gitc("add", "-A", cwd=v); gitc("commit", "-qm", "resolved", cwd=v)
    if extra_commit:
        extra_commit(v)
    r = s.mp("--resume", work, str(out))
    return r, (out.read_text() if out.exists() else "")


def replace_block(text, new):
    import re
    out, n = re.subn(r"<<<<<<< [^\n]*\n.*?>>>>>>> [^\n]*\n", new, text, flags=re.S)
    assert n == 1, text
    return out


@pytest.mark.parametrize("resolution, ok", [
    ("a line 10 (ours, theirs)\n", True),                       # both edits on one line
    ("a line 10 (ours)\na line 10 (theirs)\n", True),           # keep both lines
    ("a line 10 (theirs)\n", True),                             # take one side
    ("a line 10 (ours, theirs)\nEVIL\n", False),                # a new line below the block
    ("EVIL\na line 10 (ours, theirs)\n", False),                # a new line above it
    ("a line 10 (ours)\nEVIL\n", False),                        # one side kept, plus a new line
])
def test_resume_accepts_only_a_resolution_of_the_conflicting_lines(synth, resolution, ok):
    # app-r3 A5/H5: a line inserted right next to a marker block was accepted and written.
    r, patch = resume_case(synth, lambda t: replace_block(t, resolution))
    if ok:
        assert r.returncode == 0 and "wrote" in r.stdout, r.stdout + r.stderr
    else:
        assert r.returncode != 0 and "EVIL" not in patch, r.stdout + r.stderr
        assert "resolution:" in r.stderr, r.stderr


def test_resume_refuses_a_commit_stacked_on_the_merge(synth):
    # app-r3 A5/H13: an extra commit after the resolved merge was accepted and written.
    def extra(v):
        f = v / VPATH / "b.rs"
        f.write_text(f.read_text().replace("b line 3\n", "b line 3 EVIL\n"))
        gitc("add", "-A", cwd=v); gitc("commit", "-qm", "extra", cwd=v)
    r, patch = resume_case(synth, lambda t: replace_block(t, "a line 10 (ours, theirs)\n"), extra)
    assert r.returncode != 0 and "EVIL" not in patch, r.stdout + r.stderr
    assert "not the merge of ours and theirs" in r.stderr, r.stderr


# ---------------------------------------------------------------------------
# Round 4 (app-r4 D2, D4 and the disclosed H8).

def test_resume_refuses_a_mode_change_added_in_the_resolution(synth):
    # app-r4 R4-M1: `chmod +x` on the conflicted file passed and was written.
    def chmod(v):
        (v / VPATH / "a.rs").chmod(0o755)
        gitc("commit", "-q", "--amend", "-a", "--no-edit", cwd=v)
    r, patch = resume_case(synth, lambda t: replace_block(t, "a line 10 (ours, theirs)\n"), chmod)
    assert r.returncode != 0 and "new mode" not in patch, r.stdout + r.stderr
    assert "modes: " in r.stderr and "a.rs (100755, expected 100644)" in r.stderr, r.stderr


def test_resume_compares_parents_with_the_recorded_sides(synth):
    # app-r4 R4-M3: moving the clone's `theirs` ref to a commit adding EVIL,
    # merging it again and resolving passed, and EVIL was written.
    s = synth
    c1 = s.variant("c1", s.base, lambda: s.edit("a.rs", 10, " (ours)"))
    c2 = s.variant("c2", s.base, lambda: s.edit("a.rs", 10, " (theirs)"))
    out = s.root / "c.patch"
    out.unlink(missing_ok=True)
    r = s.mp(s.base, c1, c2, str(out))
    work = r.stderr.split("--resume ", 1)[1].split()[0]
    v = Path(work) / "v"
    run("git", "merge", "--abort", cwd=v)
    gitc("checkout", "-q", "theirs", cwd=v)
    b = v / VPATH / "b.rs"
    b.write_text(b.read_text().replace("b line 3\n", "b line 3 EVIL\n"))
    gitc("commit", "-qam", "theirs plus EVIL", cwd=v)
    gitc("checkout", "-q", "merged", cwd=v)
    run("git", "-c", "user.email=t@t", "-c", "user.name=t", "merge", "-q", "--no-edit", "theirs", cwd=v, check=False)
    f = v / VPATH / "a.rs"
    f.write_text(replace_block(f.read_text(), "a line 10 (ours, theirs)\n"))
    gitc("add", "-A", cwd=v); gitc("commit", "-qm", "resolved", cwd=v)
    r = s.mp("--resume", work, str(out))
    assert r.returncode != 0 and not out.exists(), r.stdout + r.stderr
    assert "not the merge of ours and theirs" in r.stderr, r.stderr


def test_resume_refuses_an_edited_record_of_the_sides(synth):
    # The record is outside the clone, but if it is pointed at the EVIL commit
    # too, the side's tree no longer matches the repo's patch at that rev.
    s = synth
    c1 = s.variant("c1", s.base, lambda: s.edit("a.rs", 10, " (ours)"))
    c2 = s.variant("c2", s.base, lambda: s.edit("a.rs", 10, " (theirs)"))
    out = s.root / "c.patch"
    out.unlink(missing_ok=True)
    r = s.mp(s.base, c1, c2, str(out))
    work = Path(r.stderr.split("--resume ", 1)[1].split()[0])
    v = work / "v"
    run("git", "merge", "--abort", cwd=v)
    gitc("checkout", "-q", "theirs", cwd=v)
    b = v / VPATH / "b.rs"
    b.write_text(b.read_text().replace("b line 3\n", "b line 3 EVIL\n"))
    gitc("commit", "-q", "--amend", "-a", "--no-edit", cwd=v)   # parent stays the base
    evil = gitc("rev-parse", "HEAD", cwd=v)
    gitc("checkout", "-q", "merged", cwd=v)
    run("git", "-c", "user.email=t@t", "-c", "user.name=t", "merge", "-q", "--no-edit", "theirs", cwd=v, check=False)
    f = v / VPATH / "a.rs"
    f.write_text(replace_block(f.read_text(), "a line 10 (ours, theirs)\n"))
    gitc("add", "-A", cwd=v); gitc("commit", "-qm", "resolved", cwd=v)
    rec = (work / "sides").read_text().split()
    rec[3] = evil
    (work / "sides").write_text(" ".join(rec) + "\n")
    r = s.mp("--resume", str(work), str(out))
    assert r.returncode != 0 and not out.exists(), r.stdout + r.stderr
    assert "is not the tree of" in r.stderr, r.stderr


def test_check_merge_refuses_a_reverted_base_patch_mode_change(synth):
    # app-r3 H8: the base patch makes a.rs executable, both sides keep that,
    # and a merge that drops it was accepted.
    s = synth
    x = lambda: (s.tree / VPATH / "a.rs").chmod(0o755)
    ours, theirs = (lambda: s.edit("a.rs", 30, " ours")), (lambda: s.edit("a.rs", 5, " theirs"))
    xb = s.variant("xb", s.base, x)
    o, t = s.variant("xo", xb, x, ours), s.variant("xt", xb, x, theirs)
    r = s.mp("--check-merge", s.merge_commit("x-lossy", o, t, ours, theirs))
    assert r.returncode != 0 and "a.rs (100644, expected 100755)" in r.stderr, r.stdout + r.stderr
    r = s.mp("--check-merge", s.merge_commit("x-good", o, t, x, ours, theirs))
    assert r.returncode == 0 and "modes: every file mode" in r.stdout, r.stdout + r.stderr
    # One side's own chmod is carried, and so the result may take it.
    t2 = s.variant("xt2", xb, theirs)                           # theirs drops the +x
    r = s.mp("--check-merge", s.merge_commit("x-theirs", o, t2, ours, theirs))
    assert r.returncode == 0, r.stdout + r.stderr
    # Keeping the base's +x drops theirs' change of mode.
    r = s.mp("--check-merge", s.merge_commit("x-theirs-lost", o, t2, x, ours, theirs))
    assert r.returncode != 0 and "a.rs (100755, expected 100644)" in r.stderr, r.stdout + r.stderr


def test_resume_prints_each_line_written_in_neither_side(synth):
    # app-r4 D3/R4-M2: a combined line may hold anything, by the rule. It is now shown.
    r, patch = resume_case(synth, lambda t: replace_block(t, "EVIL\n"))
    assert r.returncode == 0 and "+EVIL" in patch, r.stdout + r.stderr
    assert "a.rs conflict 1 writes a line in neither side: 'EVIL'" in r.stdout, r.stdout
    r, _ = resume_case(synth, lambda t: replace_block(t, "a line 10 (theirs)\n"))
    assert r.returncode == 0 and "in neither side" not in r.stdout, r.stdout


def test_the_resume_hint_can_be_pasted_with_a_space_in_the_out_path(synth, tmp_path):
    # app-r4 D6: the hint printed the out path unquoted, so pasting it failed with usage.
    import shlex
    s = synth
    c1 = s.variant("c1", s.base, lambda: s.edit("a.rs", 10, " (ours)"))
    c2 = s.variant("c2", s.base, lambda: s.edit("a.rs", 10, " (theirs)"))
    (tmp_path / "out dir").mkdir()
    r = s.mp(s.base, c1, c2, "out dir/c x.patch", cwd=tmp_path)
    assert r.returncode != 0 and "run: " in r.stderr, r.stdout + r.stderr
    hint = r.stderr.split("run: ", 1)[1].splitlines()[0]
    argv = shlex.split(hint)
    assert argv[1:2] == ["--resume"] and argv[3] == str(tmp_path / "out dir/c x.patch"), argv
    v = Path(argv[2]) / "v"
    f = v / VPATH / "a.rs"
    f.write_text(replace_block(f.read_text(), "a line 10 (ours, theirs)\n"))
    gitc("add", "-A", cwd=v); gitc("commit", "-qm", "resolved", cwd=v)
    r = run("bash", "-c", hint, cwd="/", env=s.env, check=False)
    assert r.returncode == 0 and (tmp_path / "out dir/c x.patch").is_file(), r.stdout + r.stderr


# ---------------------------------------------------------------------------
# Round 5 (app-r5 A2, A4, A5).

def start_conflict(s, ours_edits=(), theirs_edits=()):
    """A conflicting run on a.rs:10 plus the given edits; returns (workdir, clone, out)."""
    c1 = s.variant("c1", s.base, lambda: s.edit("a.rs", 10, " (ours)"), *ours_edits)
    c2 = s.variant("c2", s.base, lambda: s.edit("a.rs", 10, " (theirs)"), *theirs_edits)
    out = s.root / "c.patch"
    out.unlink(missing_ok=True)
    r = s.mp(s.base, c1, c2, str(out))
    assert r.returncode != 0 and "--resume" in r.stderr, r.stdout + r.stderr
    work = Path(r.stderr.split("--resume ", 1)[1].split()[0])
    return work, work / "v", out


def resolve_and_resume(s, work, v, out, before_commit=None, after_commit=None):
    f = v / VPATH / "a.rs"
    f.write_text(replace_block(f.read_text(), "a line 10 (ours, theirs)\n"))
    if before_commit:
        before_commit(v)
    gitc("add", "-A", cwd=v); gitc("commit", "-qm", "resolved", cwd=v)
    if after_commit:
        after_commit(v)
    return s.mp("--resume", str(work), str(out))


def test_resume_refuses_a_side_reverted_in_a_file_with_no_conflict(synth):
    # app-r5 A5/K5: the re-merge conflicts in a.rs by construction, and that branch
    # looked only at the names git reported, so reverting ours' b.rs edit passed.
    s = synth
    work, v, out = start_conflict(s, ours_edits=[lambda: s.edit("b.rs", 30, " (ours-b)")])
    base_b = gitc("show", f"base:{VPATH}/b.rs", cwd=v) + "\n"
    r = resolve_and_resume(s, work, v, out, lambda v: (v / VPATH / "b.rs").write_text(base_b))
    assert r.returncode != 0 and not out.exists(), r.stdout + r.stderr
    assert "ours: re-merging conflicts with the merged tree in: " + f"{VPATH}/b.rs" in r.stderr, r.stderr
    # The control: the same sides, resolved honestly, pass.
    work, v, out = start_conflict(s, ours_edits=[lambda: s.edit("b.rs", 30, " (ours-b)")])
    r = resolve_and_resume(s, work, v, out)
    assert r.returncode == 0 and "(ours-b)" in out.read_text(), r.stdout + r.stderr


def test_resume_refuses_a_file_both_sides_deleted_coming_back(synth):
    # app-r5 A5/DR4.
    s = synth
    rm_b = lambda: (s.tree / VPATH / "b.rs").unlink()
    work, v, out = start_conflict(s, ours_edits=[rm_b], theirs_edits=[rm_b])
    base_b = gitc("show", f"base:{VPATH}/b.rs", cwd=v) + "\n"
    r = resolve_and_resume(s, work, v, out, lambda v: (v / VPATH / "b.rs").write_text(base_b))
    assert r.returncode != 0 and not out.exists(), r.stdout + r.stderr
    assert f"re-merging conflicts with the merged tree in: {VPATH}/b.rs" in r.stderr, r.stderr


def test_resume_refuses_a_recorded_rev_no_ref_reaches(synth):
    # app-r5 A2/SD1: the record's THEIRS pointed at a dangling commit whose patch
    # adds EVIL, the clone's theirs rebuilt from it, and EVIL was written.
    s = synth
    work, v, out = start_conflict(s)
    rec = (work / "sides").read_text().split()
    tree = gitc("rev-parse", f"{rec[6]}^{{tree}}", cwd=s.fake)
    dangling = gitc("commit-tree", tree, "-m", "no ref reaches this", cwd=s.fake)
    rec[6] = dangling
    (work / "sides").write_text(" ".join(rec) + "\n")
    r = resolve_and_resume(s, work, v, out)
    assert r.returncode != 0 and not out.exists(), r.stdout + r.stderr
    assert f"the recorded theirs rev {dangling} is reachable from no ref" in r.stderr, r.stderr


def test_resume_refuses_a_recorded_pin_that_is_not_the_configured_one(synth):
    # app-r5 A2/SD2.
    s = synth
    work, v, out = start_conflict(s)
    rec = (work / "sides").read_text().split()
    rec[0] = gitc("rev-parse", "base", cwd=v)
    (work / "sides").write_text(" ".join(rec) + "\n")
    r = resolve_and_resume(s, work, v, out)
    assert r.returncode != 0 and "is not the configured pin" in r.stderr, r.stdout + r.stderr


def _replace_ours_a(v):
    ob = gitc("rev-parse", f"ours:{VPATH}/a.rs", cwd=v)
    text = gitc("show", ob, cwd=v).splitlines(True)
    text[29] = text[29].rstrip("\n") + " EVIL\n"
    evil = v.parent / "evil-a.rs"
    evil.write_text("".join(text) + ("\n" if not text[-1].endswith("\n") else ""))
    gitc("replace", ob, gitc("hash-object", "-w", str(evil), cwd=v), cwd=v)


def _keep_driver(v):
    (v / ".git/info/attributes").write_text("* merge=keep\n")
    gitc("config", "merge.keep.driver", "true", cwd=v)


def _gitattributes_union(v):
    (v / VPATH / ".gitattributes").write_text("* merge=union\n")


@pytest.mark.parametrize("setup, when, why", [
    (_replace_ours_a, "before", "replace refs"),                         # app-r5 A4/RP1
    (_keep_driver, "after", "info/attributes; a configured merge driver"),  # A4/AT1
    (_gitattributes_union, "before", "merge attributes in HEAD:"),       # merge settings committed
])
def test_resume_refuses_a_clone_whose_git_setup_changes_the_checks(synth, setup, when, why):
    s = synth
    work, v, out = start_conflict(s)
    evil = lambda v: (v / VPATH / "a.rs").write_text(
        (v / VPATH / "a.rs").read_text().replace("a line 29\n", "a line 29 EVIL\n"))
    if when == "before":
        r = resolve_and_resume(s, work, v, out, lambda v: (setup(v), evil(v)))
    else:
        r = resolve_and_resume(s, work, v, out, evil, setup)
    assert r.returncode != 0 and not out.exists(), r.stdout + r.stderr
    assert "changes how git reads or merges it:" in r.stderr and why in r.stderr, r.stderr


# Round 6 (app-r6 A5/AT15, AT15b): git's default global attributes file, with no
# config naming it. `* merge=binary` there made the conflicting run leave a.rs as
# ours with no markers, the resolver committed it, and theirs' a.rs:10 was lost.

@pytest.mark.parametrize("where", ["xdg", "home"])
def test_resume_refuses_merge_attributes_in_the_global_attributes_file(synth, tmp_path, where):
    s = synth
    home = tmp_path / "home"
    xdg = tmp_path / "xdg" if where == "xdg" else home / ".config"
    (xdg / "git").mkdir(parents=True)
    (xdg / "git/attributes").write_text("* merge=binary\n")
    saved = s.env
    s.env = {**saved, "HOME": str(home), "GIT_CONFIG_NOSYSTEM": "1"}
    s.env.pop("XDG_CONFIG_HOME", None)
    if where == "xdg":
        s.env["XDG_CONFIG_HOME"] = str(xdg)
    try:
        work, v, out = start_conflict(s)
        f = v / VPATH / "a.rs"
        assert "<<<<<<<" not in f.read_text() and "a line 10 (ours)\n" in f.read_text()
        gitc("add", "-A", cwd=v); gitc("commit", "-qm", "resolved as git left it", cwd=v)
        r = s.mp("--resume", str(work), str(out))
    finally:
        s.env = saved
    assert r.returncode != 0 and not out.exists(), r.stdout + r.stderr
    assert "changes how git reads or merges it:" in r.stderr and f"merge=binary on HEAD:" in r.stderr, r.stderr
