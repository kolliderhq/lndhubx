use crossbeam_channel::Sender;
use msgs::{*, bank::*};
use utils::time::time_now;
use std::{thread, time};

#[derive(Debug)]
pub struct DcaInterval {
	pub name: String,
	pub next_time_stamp: u64,
	pub duration_in_millis: u64,
}

fn get_next_timestamp(millis: u64) -> u64 {
	let now = time_now();
	return ((now / millis) + 1)* millis
}

pub async fn dca_task(listener: Sender<Message>) {

	let intervals = vec![("1d", 86400000), ("1w", 604800000), ("1m", 2419200000)];
	let mut dca_intervals = intervals.iter().map(|(name, millis)| {
		let dca_interval = DcaInterval {
			name: name.to_string(),
			next_time_stamp: get_next_timestamp(*millis),
			duration_in_millis: *millis
		};
		dca_interval
	}).collect::<Vec<DcaInterval>>();

	loop {
		let now = time_now();
		dca_intervals.iter_mut().for_each(|interval| {
			if interval.next_time_stamp < now {
				interval.next_time_stamp = get_next_timestamp(interval.duration_in_millis);
				let dca_rebalance = DcaRebalance {
					interval_name: interval.name.clone()
				};
				let msg = Message::Bank(Bank::DcaRebalance(dca_rebalance));
				listener.send(msg).unwrap();
			}
		});
		thread::sleep(time::Duration::from_millis(60000));
	}
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_dca_task() {
    }
}
