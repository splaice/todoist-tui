# tt

A full-screen terminal client for [Todoist](https://todoist.com). The left column is the account, Inbox, Today, Upcoming, and projects. The right column is the selected view: overdue and dated groups, then the tasks.

`tt` is an independent client. It is not made by Todoist.

## Run

Create a token in Todoist → Settings → Integrations → Developer, then:

```sh
export TODOIST_API_TOKEN="your-token"
cargo run --release
```

The binary is `target/release/tt`.

## Layout

The header on the left holds account actions. The header on the right holds actions for the task list. A narrow window hides the less common actions; the keys still work.

Today shows overdue tasks, then today. Upcoming shows the seven days after today. Inbox and a project show tasks from that project, grouped by date when they have one.

The section under the list shows the selected task: the full title, project, priority, due date, labels, and description. It fills the rest of the right column and stays at least 30% of that space. A longer list scrolls above it. `enter` or `>` opens that section. `↑` `↓` then scroll it, and so do `[` `]` from anywhere. The mouse wheel scrolls it while the pointer is over it. `esc` returns to the list.

Priority uses Todoist's circles: `●` red is p1, orange is p2, blue is p3, and `○` is p4. A date in red is overdue. `↻` marks a repeating task.

## Keys

| | |
| --- | --- |
| Move | `↑` `↓` `j` `k` |
| Page | `pgup` `pgdn` `ctrl-u` `ctrl-d` |
| Jump | `g` `G` `home` `end` |
| Panes | `tab` `h` `l` `←` `→` |
| Filter | `/` |
| Add task | `n` |
| Complete | `x` `space` |
| Reschedule | `e` |
| Delete | `d` |
| Open detail | `enter` `>` |
| Scroll detail | `↑` `↓` `[` `]` |
| Refresh | `r` |
| Close a dialog | `esc` |
| Quit | `q` or `ctrl-c` |
| Help | `?` |

`n` opens quick add from any pane except a text field. The same text Todoist accepts works here: `tomorrow`, `#Project`, `p1`, `@label`, and `// note`. From a project, the task is filed there unless the text names another one. Inbox, Today, and Upcoming file into the Inbox.

`/` filters the open view by task text, description, project, label, or due date.

## Tasks

`x` or `space` completes the selected task. `e` reschedules it with a natural-language date. `d` asks before deleting. `r` reloads projects and tasks from Todoist.
