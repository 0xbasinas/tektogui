# Notes for Claude Code

Read [AGENTS.md](AGENTS.md) first. It has the project rules, skills, and checks.

## Editing files

Always change files with the Edit tool, and nothing else. Do not edit files with shell commands or scripts (`sed`, `python`, heredocs, `echo >`, `Set-Content`, and similar). Read the file first, then use Edit with an exact string. If a file does not exist yet, create it with the Write tool. Shell commands are for building, testing, and running only.
