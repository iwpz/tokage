# Spec

## Goals
Review the two local documentation files.

# Tasks

```yaml
- id: T1
  title: Readme update
  goal: Review README.md and preserve its references
  domain: backend-api
  write_scope: [README.md]
  acceptance:
    commands: [git diff --check]
    criteria: [References preserved]
- id: T10
  title: Notes update
  goal: Review NOTES.md and preserve its structure
  domain: backend-api
  write_scope: [NOTES.md]
  acceptance:
    commands: [git diff --check]
    criteria: [Structure preserved]
```
