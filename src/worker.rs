use crate::app::{Job, Msg};
use crate::todoist::Client;

pub fn execute(client: &Client, job: Job) -> Msg {
    match job {
        Job::Sync { generation } => Msg::Synced {
            generation,
            result: client.sync(),
        },
        Job::QuickAdd { generation, text } => Msg::Added {
            generation,
            result: client.quick_add(&text),
        },
        Job::Close { id } => {
            let result = client.close(&id);
            Msg::Closed { id, result }
        }
        Job::Delete { generation, id } => {
            let result = client.delete(&id);
            Msg::Deleted {
                generation,
                id,
                result,
            }
        }
        Job::Reschedule {
            generation,
            id,
            due,
        } => {
            let result = client.reschedule(&id, &due);
            Msg::Rescheduled { generation, result }
        }
    }
}
