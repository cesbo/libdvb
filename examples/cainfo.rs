use {
    std::{
        os::unix::io::AsRawFd,
    },

    anyhow::Result,

    nix::{
        poll::{
            PollFd,
            PollFlags,
            poll,
        },
        sys::{
            timerfd::{
                ClockId,
                Expiration,
                TimerFd,
                TimerFlags,
                TimerSetTimeFlags,
            },
            time::{
                TimeSpec,
                TimeValLike,
            },
        },
    },

    libdvb::{
        CaDevice,
    },
};


fn start_ca(adapter: u32, device: u32) -> Result<()> {
    // let mut ca = CaDevice::open(path, 0)?;

    let timer = TimerFd::new(
        ClockId::CLOCK_MONOTONIC,
        TimerFlags::all()
    )?;

    timer.set(
        Expiration::Interval(
            TimeSpec::milliseconds(100)
        ),
        TimerSetTimeFlags::empty()
    )?;

    let mut pool: Vec<PollFd> = Vec::new();

    pool.push(PollFd::new(
        timer.as_raw_fd(),
        PollFlags::POLLIN
    ));

    let instant = std::time::Instant::now();

    for _ in 0 .. 10 {
        let mut total = poll(&mut pool, -1)?;
        // less than 0 not needed to check we got error in this case
        // equal to 0 not needed to check we have no timeout

        for (i, item) in pool.iter().enumerate() {
            let revent = item.revents().unwrap_or_else(PollFlags::empty);
            if revent.is_empty() {
                continue;
            }

            if i == 0 {
                timer.wait()?;
                dbg!(instant.elapsed());
            }

            if total > 1 {
                total -= 1;
            } else {
                break;
            }
        }

        // loop
    }

    // let fd = PollFd::new(
    //     ca.as_raw_fd(),
    //     PollFlags::POLLIN,
    // );

    // TODO: CaPool
    // TODO: timer CA_DELAY -> poll_timer()
    // TODO: self.as_raw_fd() -> poll_event()

    Ok(())
}


fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let adapter = match args.next() {
        Some(v) => v.parse::<u32>().context("adapter number")?,
        None => bail!("adapter number not defined"),
    };

    let device = match args.next() {
        Some(v) => v.parse::<u32>().context("device number")?,
        None => 0,
    };

    start_ca(adapter, device)
}
