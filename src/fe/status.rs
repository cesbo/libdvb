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


/// Helper struct for displaying frontend status
pub struct FeStatusDisplay<'a> {
    inner: &'a FeStatus,
    verbose: u32,
}


impl<'a> FeStatusDisplay<'a> {
    fn display_0(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Status:")?;

        if self.inner.status == FE_NONE {
            write!(f, "OFF")?;
            return Ok(());
        }

        const STATUS_MAP: &[char] = &['S', 'C', 'V', 'Y', 'L'];
        for (i, s) in STATUS_MAP.iter().enumerate() {
            let c = if self.inner.status & (1 << i) != 0 { *s } else { '_' };
            write!(f, "{}", c)?;
        }

        if self.inner.status & FE_HAS_SIGNAL == 0 {
            return Ok(());
        }

        write!(f, " S:")?;
        if let Some((decibel, relative)) = self.inner.get_signal_level() {
            write!(f, "{:.02}dBm ({:.0}%)", decibel, relative)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_CARRIER == 0 {
            return Ok(());
        }

        write!(f, " Q:")?;
        if let Some((decibel, relative)) = self.inner.get_signal_noise_ratio() {
            write!(f, "{:.02}dB ({}%)", decibel, relative)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_LOCK == 0 {
            return Ok(());
        }

        write!(f, " BER:")?;
        if let Some(ber) = self.inner.props[2].get_stats_counter() {
            write!(f, "{}", ber & 0xFFFF)?;
        } else {
            write!(f, "-")?;
        }

        write!(f, " UNC:")?;
        if let Some(unc) = self.inner.props[3].get_stats_counter() {
            write!(f, "{}", unc & 0xFFFF)
        } else {
            write!(f, "-")
        }
    }

    fn display_1(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Status:")?;

        if self.inner.status == FE_NONE {
            write!(f, " OFF")?;
            return Ok(());
        }

        const STATUS_MAP: &[&str] = &[
            "SIGNAL", "CARRIER", "FEC", "SYNC", "LOCK", "TIMEOUT", "REINIT"
        ];
        for (i, s) in STATUS_MAP.iter().enumerate() {
            if self.inner.status & (1 << i) != 0 {
                write!(f, " {}", s)?;
            }
        }

        if self.inner.status & FE_HAS_SIGNAL == 0 {
            return Ok(());
        }

        write!(f, "\nSignal: ")?;
        if let Some((decibel, relative)) = self.inner.get_signal_level() {
            write!(f, "{:.02}dBm ({:.0}%)", decibel, relative)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_CARRIER == 0 {
            return Ok(());
        }

        write!(f, "\nSNR: ")?;
        if let Some((decibel, relative)) = self.inner.get_signal_noise_ratio() {
            write!(f, "{:.02}dB ({}%)", decibel, relative)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_LOCK == 0 {
            return Ok(());
        }

        write!(f, "\nBER: ")?;
        if let Some(ber) = self.inner.props[2].get_stats_counter() {
            write!(f, "{}", ber & 0xFFFF)?;
        } else {
            write!(f, "-")?;
        }

        write!(f, "\nUNC: ")?;
        if let Some(unc) = self.inner.props[3].get_stats_counter() {
            write!(f, "{}", unc & 0xFFFF)
        } else {
            write!(f, "-")
        }
    }
}


impl<'a> fmt::Display for FeStatusDisplay<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.verbose {
            1 => self.display_1(f),
            _ => self.display_0(f),
        }
    }
}


impl FeStatus {
    /// Returns an object that implements `Display` for different verbosity levels
    /// Verbosity levels:
    /// 0 - single line status
    /// ```text
    /// Status:SCVYL S:-38.56dBm (59%) Q:14.57dB (70%) BER:0 UNC:0
    /// ```
    ///
    /// 1 - full report
    /// ```text
    /// Status: SIGNAL CARRIER FEC SYNC LOCK
    /// Signal: -38.20dBm (59%)
    /// SNR: 14.65dB (70%)
    /// BER: 0
    /// UNC: 0
    /// ```
    pub fn display(&self, verbose: u32) -> FeStatusDisplay {
        FeStatusDisplay {
            inner: self,
            verbose,
        }
    }

    fn get_signal_level(&self) -> Option<(f64, u64)> {
        // TODO: config for lo/hi
        // let lo: f64 = -85.0;
        // let hi: f64 = -6.0;
        // let relative = 100.0 - (decibel - hi) * 100.0 / (lo - hi);
        None
    }

    fn get_signal_noise_ratio(&self) -> Option<(f64, u64)> {
        // let relative = 5 * decibel as u32;
        None
    }

    /// Reads frontend status
    pub fn read(&mut self, fe: &FeDevice) -> Result<()> {
        self.status = FE_NONE;

        // FE_READ_STATUS
        ioctl_read!(#[inline] ioctl_call, b'o', 69, u32);
        unsafe {
            ioctl_call(fe.as_raw_fd(), &mut self.status as *mut _)
        }.context("frontend read status")?;

        if self.status == FE_NONE {
            return Ok(());
        }

        fe.get_properties(&mut self.props)?;

        Ok(())
    }
}
