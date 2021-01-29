use {
    std::{
        fmt,
        os::unix::io::AsRawFd,
    },

    anyhow::{
        Context,
        Result,
    },

    nix::{
        ioctl_read,
    },

    super::{
        FeDevice,
        sys::*,
    },
};


/// Frontend status
#[derive(Debug)]
pub struct FeStatus {
    /// `sys::frontend::fe_status`
    status: u32,

    /// properties
    props: [DtvProperty; 4],
}


impl Default for FeStatus {
    fn default() -> FeStatus {
        FeStatus {
            status: 0,
            props: [
                // 0: signal level
                DtvProperty::new(DTV_STAT_SIGNAL_STRENGTH, 0),
                // 1: signal-to-noise ratio
                DtvProperty::new(DTV_STAT_CNR, 0),
                // 2: ber - number of bit errors
                DtvProperty::new(DTV_STAT_PRE_ERROR_BIT_COUNT, 0),
                // 3: unc - number of block errors
                DtvProperty::new(DTV_STAT_ERROR_BLOCK_COUNT, 0),
            ],
        }
    }
}


/// Returns an object that implements `Display` for different verbosity levels
///
/// ```text
/// Status:SCVYL S:-38.56dBm (59%) Q:14.57dB (70%) BER:0 UNC:0
/// ```
///
/// Status:
/// - S - Signal
/// - C - Carrier
/// - V - FEC
/// - Y - Sync
/// - L - Lock
impl fmt::Display for FeStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Status:")?;

        if self.status == FE_NONE {
            write!(f, "OFF")?;
            return Ok(());
        }

        const STATUS_MAP: &[char] = &['S', 'C', 'V', 'Y', 'L'];
        for (i, s) in STATUS_MAP.iter().enumerate() {
            let c = if self.status & (1 << i) != 0 { *s } else { '_' };
            write!(f, "{}", c)?;
        }

        if self.status & FE_HAS_SIGNAL == 0 {
            return Ok(());
        }

        write!(f, " S:")?;
        if let Some(s) = self.get_signal_level() {
            // TODO: config for lo/hi
            let lo: f64 = -85.0;
            let hi: f64 = -6.0;
            let relative = 100.0 - (s - hi) * 100.0 / (lo - hi);
            write!(f, "{:.02}dBm ({:.0}%)", s, relative)?;
        } else {
            write!(f, "-")?;
        }

        if self.status & FE_HAS_CARRIER == 0 {
            return Ok(());
        }

        write!(f, " Q:")?;
        if let Some(q) = self.get_signal_noise_ratio() {
            let relative = q * 2.;
            write!(f, "{:.02}dB ({:.0}%)", q, relative)?;
        } else {
            write!(f, "-")?;
        }

        if self.status & FE_HAS_LOCK == 0 {
            return Ok(());
        }

        write!(f, " BER:")?;
        if let Some(ber) = self.get_ber() {
            write!(f, "{}", ber)?;
        } else {
            write!(f, "-")?;
        }

        write!(f, " UNC:")?;
        if let Some(unc) = self.get_unc() {
            write!(f, "{}", unc)?;
        } else {
            write!(f, "-")?;
        }

        Ok(())
    }
}


impl FeStatus {
    fn get_stats_decibel(&self, u: usize) -> Option<f64> {
        let stats = self.props.get(u)?;
        let stats = unsafe { &stats.u.st };

        let len = ::std::cmp::min(stats.len as usize, stats.stat.len());
        for s in stats.stat[.. len].iter() {
            if s.scale == FE_SCALE_DECIBEL {
                return Some((s.value as f64) / 1000.0);
            }
        }

        None
    }

    /// Returns Signal Level in dBm
    #[inline]
    pub fn get_signal_level(&self) -> Option<f64> { self.get_stats_decibel(0) }

    /// Returns Signal to noise ratio in dB
    #[inline]
    pub fn get_signal_noise_ratio(&self) -> Option<f64> { self.get_stats_decibel(1) }

    fn get_stats_counter(&self, u: usize) -> Option<u32> {
        let stats = self.props.get(u)?;
        let stats = unsafe { &stats.u.st };
        if stats.len > 0 {
            let s = &stats.stat[0];
            if s.scale == FE_SCALE_COUNTER {
                return Some((s.value & 0xFFFF) as u32);
            }
        }
        None
    }

    /// Returns BER value if available
    #[inline]
    pub fn get_ber(&self) -> Option<u32> { self.get_stats_counter(2) }

    /// Returns UNC value if available
    #[inline]
    pub fn get_unc(&self) -> Option<u32> { self.get_stats_counter(3) }

    /// Reads frontend status
    pub fn read(&mut self, fe: &FeDevice) -> Result<()> {
        self.status = FE_NONE;

        // FE_READ_STATUS
        ioctl_read!(#[inline] ioctl_call, b'o', 69, u32);
        unsafe {
            ioctl_call(fe.as_raw_fd(), &mut self.status as *mut _)
        }.context("FE: read status")?;

        if self.status == FE_NONE {
            return Ok(());
        }

        fe.get_properties(&mut self.props)?;

        Ok(())
    }
}
