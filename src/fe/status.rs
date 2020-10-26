use {
    std::fmt,

    anyhow::Result,

    crate::{
        sys::frontend::*,
        FeDevice,
    },
};


/// Frontend status
#[derive(Default, Debug, Copy, Clone)]
pub struct FeStatus {
    /// `sys::frontend::fe_status`
    status: u32,

    /// signal level in dBm
    signal: Option<f64>,

    /// signal-to-noise ratio in dB
    snr: Option<f64>,

    /// number of bit errors before the forward error correction coding
    ber: Option<u64>,

    /// number of block errors after the outer forward error correction coding
    unc: Option<u64>,
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
        if let Some(signal) = self.inner.signal {
            // TODO: config for lo/hi
            let lo: f64 = -85.0;
            let hi: f64 = -6.0;
            let relative = 100.0 - (signal - hi) * 100.0 / (lo - hi);
            write!(f, "{:.0}% ({:.02}dBm)", relative, signal)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_CARRIER == 0 {
            return Ok(());
        }

        write!(f, " Q:")?;
        if let Some(snr) = self.inner.snr {
            let relative = 5 * snr as u32;
            write!(f, "{}% ({:.02}dB)", relative, snr)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_LOCK == 0 {
            return Ok(());
        }

        write!(f, " BER: ")?;
        if let Some(ber) = self.inner.ber {
            write!(f, "{}", ber & 0xFFFF)?;
        } else {
            write!(f, "-")?;
        }

        write!(f, " UNC: ")?;
        if let Some(unc) = self.inner.unc {
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
        if let Some(signal) = self.inner.signal {
            // TODO: config for lo/hi
            let lo: f64 = -85.0;
            let hi: f64 = -6.0;
            let relative = 100.0 - (signal - hi) * 100.0 / (lo - hi);
            write!(f, "{:.0}% ({:.02}dBm)", relative, signal)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_CARRIER == 0 {
            return Ok(());
        }

        write!(f, "\nSNR: ")?;
        if let Some(snr) = self.inner.snr {
            let relative = 5 * snr as u32;
            write!(f, "{}% ({:.02}dB)", relative, snr)?;
        } else {
            write!(f, "-")?;
        }

        if self.inner.status & FE_HAS_LOCK == 0 {
            return Ok(());
        }

        write!(f, "\nBER: ")?;
        if let Some(ber) = self.inner.ber {
            write!(f, "{}", ber & 0xFFFF)?;
        } else {
            write!(f, "-")?;
        }

        write!(f, "\nUNC: ")?;
        if let Some(unc) = self.inner.unc {
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
    /// 1 - full report
    pub fn display(&self, verbose: u32) -> FeStatusDisplay {
        FeStatusDisplay {
            inner: self,
            verbose,
        }
    }

    /// Reads frontend status
    pub fn read(&mut self, fe: &FeDevice) -> Result<()> {
        self.status = FE_NONE;
        fe.ioctl(FE_READ_STATUS, &mut self.status as *mut _)?;

        if self.status == FE_NONE {
            return Ok(());
        }

        let mut cmdseq = [
            DtvProperty::new(DTV_STAT_SIGNAL_STRENGTH, 0),
            DtvProperty::new(DTV_STAT_CNR, 0),
            DtvProperty::new(DTV_STAT_PRE_ERROR_BIT_COUNT, 0),
            DtvProperty::new(DTV_STAT_ERROR_BLOCK_COUNT, 0),
        ];
        let mut cmd = DtvProperties::new(&mut cmdseq);

        fe.ioctl(FE_GET_PROPERTY, cmd.as_mut_ptr())?;

        self.signal = (unsafe { cmdseq[0].u.st }).get_decibel();
        self.snr = (unsafe { cmdseq[1].u.st }).get_decibel();
        self.ber = (unsafe { cmdseq[2].u.st }).get_counter();
        self.unc = (unsafe { cmdseq[3].u.st }).get_counter();

        Ok(())
    }
}
