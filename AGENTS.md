# `notes` branch

This is a **permanent, never-merged scratch branch** for freeform working notes,
investigation write-ups, bug analyses, and implementation plans that should live
in version control but must never appear in `main` or any feature branch.

## Rules

- **Never merge this branch into any other branch.**
- Files here are for human and agent reference only – not shipped code.
- Add notes freely; no commit-message ceremony required.
- Keep all note files under `notes/` so the tree stays tidy.

## Why an orphan branch?

An orphan branch has no common ancestor with `main`, so it is structurally
impossible for `git merge` to fast-forward or accidentally pull these files
into the codebase. `git log --all` still shows the branch, so notes are
discoverable, but `git diff main` and PR diffs stay clean.

## How to use

```bash
# Read a note without switching branches
git show notes:notes/some-note.md

# Add a note from any branch
git worktree add /tmp/ccm-notes notes   # or just: git checkout notes
# ... edit notes/ ...
git add notes/my-note.md
git commit -m "notes: add my-note"
git checkout -                          # back to previous branch
```
