//! Job events and the per-job buffer that makes watch streams resumable.
//!
//! Every change to a job is an [`Event`] with an id from one counter shared
//! by all jobs, so ids are unique and increase in the order the changes
//! happened. A watch stream sends a [`Snapshot`] first and events after it.
//! The snapshot names the id of the last event it includes; the client
//! remembers the id of the last message it saw and, after a disconnect,
//! asks to resume from there.
//!
//! Each job keeps its last [`EVENT_BUFFER`] events in an `EventBuffer`.
//! Resuming from `after` is possible when nothing newer than `after` has
//! been evicted from the buffers the stream covers (`can_resume`); the
//! stream then replays the buffered events with a higher id and goes on
//! live. Otherwise it starts over with a fresh snapshot, which is always
//! correct, just more data.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::SystemTime;

use super::{Job, JobError, JobId, JobProgress, JobState};

/// How many events each job keeps for resuming watch streams. ffmpeg
/// reports progress twice a second, so this covers several minutes of
/// disconnect for a job that logs little.
pub const EVENT_BUFFER: usize = 1024;

/// One change to one job.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
	/// Unique and increasing across all jobs.
	pub id: u64,
	/// The job that changed.
	pub job: JobId,
	/// What changed.
	pub kind: EventKind,
}

/// What an [`Event`] reports.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
	/// The job moved to `state`.
	State {
		/// The new state.
		state: JobState,
		/// Why, when the new state is [`JobState::Failed`].
		error: Option<JobError>,
		/// When it happened.
		at: SystemTime,
	},
	/// ffmpeg reported progress.
	Progress(JobProgress),
	/// ffmpeg wrote one line to stderr.
	Log(String),
}

impl Event {
	/// `true` for a state event that ends the job.
	#[must_use]
	pub fn is_terminal(&self) -> bool {
		matches!(self.kind, EventKind::State { state, .. } if state.is_terminal())
	}
}

/// The full state a watch stream starts with. A job watch holds one job,
/// a queue watch every job. Events with an id at or below `event_id` are
/// already included.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
	/// The id of the newest event at the time of the snapshot.
	pub event_id: u64,
	/// The jobs, oldest first.
	pub jobs: Vec<Job>,
}

/// One message of a watch stream.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchItem {
	/// The state to start from. The first message of a fresh stream, and
	/// sent again when the client fell too far behind to resume.
	Snapshot(Snapshot),
	/// A change since the snapshot or the previous event.
	Event(Arc<Event>),
}

impl WatchItem {
	/// The event id a client should remember to resume after this message.
	#[must_use]
	pub fn event_id(&self) -> u64 {
		match self {
			Self::Snapshot(snapshot) => snapshot.event_id,
			Self::Event(event) => event.id,
		}
	}
}

/// The last events of one job, oldest first.
#[derive(Debug)]
pub(crate) struct EventBuffer {
	events: VecDeque<Arc<Event>>,
	capacity: usize,
	/// The id of the newest event pushed out of the buffer; 0 when none was.
	evicted: u64,
}

impl EventBuffer {
	pub(crate) fn new(capacity: usize) -> Self {
		Self {
			events: VecDeque::with_capacity(capacity.min(64)),
			capacity: capacity.max(1),
			evicted: 0,
		}
	}

	/// Appends `event`, dropping the oldest one when full. Events must be
	/// pushed in id order.
	pub(crate) fn push(&mut self, event: Arc<Event>) {
		if self.events.len() == self.capacity
			&& let Some(oldest) = self.events.pop_front()
		{
			self.evicted = oldest.id;
		}
		self.events.push_back(event);
	}

	/// The id of the newest event no longer in the buffer; 0 when every
	/// event ever pushed is still here.
	pub(crate) fn newest_evicted(&self) -> u64 {
		self.evicted
	}

	/// The buffered events with an id above `after`, oldest first.
	pub(crate) fn after(&self, after: u64) -> impl Iterator<Item = &Arc<Event>> {
		let start = self.events.partition_point(|e| e.id <= after);
		self.events.range(start..)
	}
}

/// Whether a client that last saw event `after` can be given the events it
/// missed. `evicted` is the newest id dropped from the buffers the stream
/// covers and `latest` the newest id handed out. `after` must be a real id
/// (never 0, which asks for a snapshot), not from the future, and nothing
/// the client has not seen may have been evicted.
#[must_use]
pub(crate) fn can_resume(after: u64, evicted: u64, latest: u64) -> bool {
	after != 0 && after >= evicted && after <= latest
}

#[cfg(test)]
mod tests {
	use super::*;

	fn event(id: u64) -> Arc<Event> {
		Arc::new(Event {
			id,
			job: JobId::from("j1"),
			kind: EventKind::Log(format!("line {id}")),
		})
	}

	fn ids<'a>(events: impl Iterator<Item = &'a Arc<Event>>) -> Vec<u64> {
		events.map(|e| e.id).collect()
	}

	#[test]
	fn buffer_keeps_the_newest_events_and_remembers_what_it_dropped() {
		let mut buffer = EventBuffer::new(3);
		assert_eq!(buffer.newest_evicted(), 0);
		for id in 1..=5 {
			buffer.push(event(id));
		}
		assert_eq!(ids(buffer.after(0)), vec![3, 4, 5]);
		assert_eq!(buffer.newest_evicted(), 2);
	}

	#[test]
	fn after_skips_what_the_client_saw() {
		let mut buffer = EventBuffer::new(8);
		// Ids are global, so a job's buffer has gaps.
		for id in [2, 5, 6, 9] {
			buffer.push(event(id));
		}
		assert_eq!(ids(buffer.after(5)), vec![6, 9]);
		assert_eq!(ids(buffer.after(7)), vec![9]);
		assert_eq!(ids(buffer.after(9)), Vec::<u64>::new());
		assert_eq!(ids(buffer.after(0)), vec![2, 5, 6, 9]);
	}

	#[test]
	fn resume_rule() {
		// Nothing evicted: any id handed out so far works.
		assert!(can_resume(1, 0, 10));
		assert!(can_resume(10, 0, 10));
		// 0 asks for a snapshot, and ids from the future are unknown.
		assert!(!can_resume(0, 0, 10));
		assert!(!can_resume(11, 0, 10));
		// The client must have seen everything that was evicted.
		assert!(can_resume(4, 4, 10));
		assert!(!can_resume(3, 4, 10));
	}

	#[test]
	fn a_watch_item_names_the_id_to_resume_after() {
		let snapshot = WatchItem::Snapshot(Snapshot {
			event_id: 7,
			jobs: Vec::new(),
		});
		assert_eq!(snapshot.event_id(), 7);
		assert_eq!(WatchItem::Event(event(8)).event_id(), 8);
		assert!(!event(8).is_terminal());
	}
}
