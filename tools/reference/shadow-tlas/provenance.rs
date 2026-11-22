//! Submission-order provenance model for a future GPU-visible BLAS directory.
//!
//! This is an offline correctness control, not a runtime implementation. It
//! models only recorded deserialize writes, reads, and already-enqueued fence
//! signals/waits. It rejects unordered conflicting accesses and unknown waits.
//! Actual runtime use additionally needs resource ownership, GPU publication,
//! alias tracking, submission failure handling, and fence-based retirement.
use std::collections::BTreeMap;

type Queue = u32;
type Fence = u32;
type Address = u64;
type Version = u64;
type Clock = BTreeMap<Queue, u64>;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Error {
    UnknownQueue,
    UnknownFence,
    FenceOrder,
    UnorderedAccess,
    UnknownAddress,
    Unsupported,
    UnknownSource,
    InvalidRange,
    Alias,
}

fn before(a: &Clock, b: &Clock) -> bool {
    a.iter()
        .all(|(queue, value)| *value <= *b.get(queue).unwrap_or(&0))
}

#[derive(Clone)]
struct Write {
    clock: Clock,
    version: Version,
}
#[derive(Clone, Default)]
struct Entry {
    writer: Option<Write>,
    readers: Vec<Clock>,
}
#[derive(Clone, Default)]
struct Graph {
    queues: BTreeMap<Queue, Clock>,
    fences: BTreeMap<Fence, BTreeMap<u64, Clock>>,
    addresses: BTreeMap<Address, Entry>,
    sources: BTreeMap<u64, Version>,
    ranges: BTreeMap<Address, u64>,
}

#[derive(Clone, Copy)]
enum Op {
    Deserialize(Address, Version),
    DeserializeFrom(Address, u64),
    Read(Address),
    Unsupported,
}

impl Graph {
    fn add_queue(&mut self, queue: Queue) {
        self.queues.entry(queue).or_default();
    }

    // Exact base-address lookups only. Reject overlapping extents rather than
    // guessing which placed resource or suballocation owns the address.
    fn register(&mut self, start: Address, bytes: u64) -> Result<(), Error> {
        if start == 0 || start % 256 != 0 || bytes == 0 {
            return Err(Error::InvalidRange);
        }
        let end = start.checked_add(bytes).ok_or(Error::InvalidRange)?;
        if self
            .ranges
            .iter()
            .any(|(&other, &other_end)| start < other_end && other < end)
        {
            return Err(Error::Alias);
        }
        self.ranges.insert(start, end);
        Ok(())
    }

    fn tick(&mut self, queue: Queue) -> Result<Clock, Error> {
        let clock = self.queues.get_mut(&queue).ok_or(Error::UnknownQueue)?;
        let value = clock.entry(queue).or_default();
        *value = value.checked_add(1).ok_or(Error::Unsupported)?;
        Ok(clock.clone())
    }

    // Treat a batch as one validation transaction: failed publication does not
    // leave part of a rejected list visible in the proof model.
    fn submit(&mut self, queue: Queue, ops: &[Op]) -> Result<Vec<Version>, Error> {
        let mut staged = self.clone();
        let mut reads = Vec::new();
        for op in ops {
            let op = match *op {
                Op::DeserializeFrom(address, source) => Op::Deserialize(
                    address,
                    *staged.sources.get(&source).ok_or(Error::UnknownSource)?,
                ),
                other => other,
            };
            let clock = staged.tick(queue)?;
            match op {
                Op::Deserialize(address, version) => {
                    if !staged.ranges.contains_key(&address) {
                        return Err(Error::UnknownAddress);
                    }
                    let entry = staged.addresses.entry(address).or_default();
                    if entry
                        .writer
                        .as_ref()
                        .is_some_and(|v| !before(&v.clock, &clock))
                        || entry.readers.iter().any(|reader| !before(reader, &clock))
                    {
                        return Err(Error::UnorderedAccess);
                    }
                    entry.writer = Some(Write { clock, version });
                    entry.readers.clear();
                }
                Op::Read(address) => {
                    let entry = staged
                        .addresses
                        .get_mut(&address)
                        .ok_or(Error::UnknownAddress)?;
                    let writer = entry.writer.as_ref().ok_or(Error::UnknownAddress)?;
                    if !before(&writer.clock, &clock) {
                        return Err(Error::UnorderedAccess);
                    }
                    reads.push(writer.version);
                    entry.readers.push(clock);
                }
                Op::Unsupported => return Err(Error::Unsupported),
                Op::DeserializeFrom(_, _) => unreachable!(),
            }
        }
        *self = staged;
        Ok(reads)
    }

    // A signal is enqueued after all preceding queue operations; it need not yet
    // have completed on the GPU. Only monotonically increasing values are in
    // this bounded model. Fences with future/unknown signals are unsupported.
    fn signal(&mut self, queue: Queue, fence: Fence, value: u64) -> Result<(), Error> {
        if self
            .fences
            .get(&fence)
            .and_then(|f| f.last_key_value())
            .is_some_and(|(&last, _)| value <= last)
        {
            return Err(Error::FenceOrder);
        }
        let clock = self.tick(queue)?;
        self.fences.entry(fence).or_default().insert(value, clock);
        Ok(())
    }

    fn wait(&mut self, queue: Queue, fence: Fence, value: u64) -> Result<(), Error> {
        let published = self
            .fences
            .get(&fence)
            .and_then(|f| f.range(value..).next())
            .map(|(_, c)| c.clone())
            .ok_or(Error::UnknownFence)?;
        let current = self.queues.get_mut(&queue).ok_or(Error::UnknownQueue)?;
        for (source, tick) in published {
            current
                .entry(source)
                .and_modify(|v| *v = (*v).max(tick))
                .or_insert(tick);
        }
        self.tick(queue)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn graph() -> Graph {
        let mut g = Graph::default();
        g.add_queue(1);
        g.add_queue(2);
        g.register(0x1000, 256).unwrap();
        g.register(0x2000, 256).unwrap();
        g
    }

    #[test]
    fn reversed_recording_order_uses_submission_order() {
        let mut g = graph();
        let first_recorded = [Op::Deserialize(0x1000, 10)];
        let second_recorded = [Op::Deserialize(0x1000, 20)];
        g.submit(1, &second_recorded).unwrap();
        g.submit(1, &first_recorded).unwrap();
        assert_eq!(g.submit(1, &[Op::Read(0x1000)]).unwrap(), [10]);
    }

    #[test]
    fn cross_queue_requires_ordered_signal_and_wait() {
        let mut g = graph();
        g.submit(1, &[Op::Deserialize(0x1000, 10)]).unwrap();
        assert_eq!(
            g.submit(2, &[Op::Read(0x1000)]),
            Err(Error::UnorderedAccess)
        );
        g.signal(1, 7, 1).unwrap();
        g.wait(2, 7, 1).unwrap();
        assert_eq!(g.submit(2, &[Op::Read(0x1000)]).unwrap(), [10]);
        assert_eq!(
            g.submit(1, &[Op::Deserialize(0x1000, 20)]),
            Err(Error::UnorderedAccess)
        );
        g.signal(2, 8, 1).unwrap();
        g.wait(1, 8, 1).unwrap();
        g.submit(1, &[Op::Deserialize(0x1000, 20)]).unwrap();
    }

    #[test]
    fn replay_publishes_new_generation_and_keeps_other_addresses() {
        let mut g = graph();
        // One recorded list refers to a mutable source token. The harness
        // supplies the source generation known at each queue execution.
        let list = [Op::DeserializeFrom(0x1000, 1), Op::Read(0x1000)];
        g.submit(1, &[Op::Deserialize(0x2000, 30)]).unwrap();
        g.sources.insert(1, 10);
        assert_eq!(g.submit(1, &list).unwrap(), [10]);
        // Caller must additionally prove completion before resubmitting a
        // direct list. This ordering-only model does not own GPU fences.
        g.sources.insert(1, 11);
        assert_eq!(g.submit(1, &list).unwrap(), [11]);
        assert_eq!(g.submit(1, &[Op::Read(0x2000)]).unwrap(), [30]);
    }

    #[test]
    fn rejection_is_transactional_and_unsupported_is_explicit() {
        let mut g = graph();
        assert_eq!(
            g.submit(1, &[Op::Deserialize(0x1000, 10), Op::Unsupported]),
            Err(Error::Unsupported)
        );
        assert_eq!(g.submit(1, &[Op::Read(0x1000)]), Err(Error::UnknownAddress));
        assert_eq!(g.wait(2, 7, 1), Err(Error::UnknownFence));
        assert_eq!(
            g.submit(99, &[Op::Deserialize(0x1000, 10)]),
            Err(Error::UnknownQueue)
        );
    }
    #[test]
    fn unordered_writes_and_future_fence_are_rejected() {
        let mut g = graph();
        g.submit(1, &[Op::Deserialize(0x1000, 10)]).unwrap();
        assert_eq!(
            g.submit(2, &[Op::Deserialize(0x1000, 20)]),
            Err(Error::UnorderedAccess)
        );
        assert_eq!(g.wait(2, 7, 2), Err(Error::UnknownFence));
        g.signal(1, 7, 1).unwrap();
        assert_eq!(g.signal(1, 7, 1), Err(Error::FenceOrder));
        assert_eq!(g.wait(2, 7, 2), Err(Error::UnknownFence));
        g.wait(2, 7, 1).unwrap();
        g.submit(2, &[Op::Deserialize(0x1000, 20)]).unwrap();
        assert_eq!(
            g.submit(1, &[Op::Read(0x1000)]),
            Err(Error::UnorderedAccess)
        );
        g.signal(2, 7, 2).unwrap();
        g.wait(1, 7, 2).unwrap();
        assert_eq!(g.submit(1, &[Op::Read(0x1000)]).unwrap(), [20]);
    }
    #[test]
    fn invalid_and_overlapping_ranges_are_rejected() {
        let mut g = graph();
        assert_eq!(g.register(0, 256), Err(Error::InvalidRange));
        assert_eq!(g.register(0x1201, 256), Err(Error::InvalidRange));
        assert_eq!(g.register(0x1200, 0), Err(Error::InvalidRange));
        assert_eq!(g.register(u64::MAX - 255, 512), Err(Error::InvalidRange));
        assert_eq!(g.register(0x1000, 256), Err(Error::Alias));
        assert_eq!(g.register(0x0f00, 512), Err(Error::Alias));
        g.register(0x1100, 256).unwrap();
        assert_eq!(
            g.submit(1, &[Op::Deserialize(0x1080, 2)]),
            Err(Error::UnknownAddress)
        );
    }
}
