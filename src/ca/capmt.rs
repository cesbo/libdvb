//! EN 50221 CA_PMT coding from an MPEG-TS PMT section.

use libmpegts::psi::{
    CaDescriptorRef,
    DescriptorsRef,
    PmtSectionRef,
};

use crate::error::{
    Error,
    Result,
};

/// en50221 8.4.3.4: ca_pmt_list_management.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum CaPmtListManagement {
    Only = 0x03,
    Add = 0x04,
    Update = 0x05,
}

/// en50221 8.4.3.4: ca_pmt_cmd_id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum CaPmtCommand {
    OkDescrambling = 0x01,
    NotSelected = 0x04,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaDescriptor {
    caid: u16,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProgramStream {
    stream_type: u8,
    elementary_pid: u16,
    descriptors: Vec<CaDescriptor>,
}

/// Owned subset of a PMT needed to construct CA_PMT objects.
///
/// Astra passes a borrowed raw PMT section through FFI, so the controller
/// must retain everything needed for updates, removal and CAM reconnection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Program {
    program_number: u16,
    version: u8,
    descriptors: Vec<CaDescriptor>,
    streams: Vec<ProgramStream>,
}

impl Program {
    /// Parses and owns one complete raw PMT section, including CRC32.
    pub fn parse(section: &[u8]) -> Result<Self> {
        let pmt = PmtSectionRef::try_from(section)
            .map_err(|error| Error::InvalidData(format!("invalid PMT section: {error}")))?;
        let program_number = pmt.program_number();
        if program_number == 0 {
            return Err(Error::InvalidData(
                "invalid PMT section: program number is zero".to_owned(),
            ));
        }

        let descriptors = parse_ca_descriptors(program_number, pmt.program_descriptors())?;
        let mut streams = Vec::new();
        for stream in pmt.streams() {
            let stream = stream.map_err(|error| {
                Error::InvalidData(format!(
                    "invalid PMT section for program {program_number}: {error}"
                ))
            })?;
            streams.push(ProgramStream {
                stream_type: stream.stream_type(),
                elementary_pid: stream.elementary_pid(),
                descriptors: parse_ca_descriptors(program_number, stream.stream_descriptors())?,
            });
        }

        Ok(Program {
            program_number,
            version: pmt.version(),
            descriptors,
            streams,
        })
    }

    pub fn program_number(&self) -> u16 {
        self.program_number
    }

    /// Builds one CA_PMT APDU body for a particular CA resource session.
    /// `None` means the PMT has no CA descriptors matching that session.
    pub fn build_ca_pmt(
        &self,
        caids: &[u16],
        list_management: CaPmtListManagement,
        command: CaPmtCommand,
    ) -> Result<Option<Vec<u8>>> {
        let mut body = Vec::new();
        body.push(list_management as u8);
        body.extend_from_slice(&self.program_number.to_be_bytes());
        body.push(0xC1 | ((self.version & 0x1F) << 1));

        let mut matched = append_info(&mut body, &self.descriptors, caids, command)?;
        for stream in &self.streams {
            body.push(stream.stream_type);
            body.extend_from_slice(&(0xE000 | stream.elementary_pid).to_be_bytes());
            matched |= append_info(&mut body, &stream.descriptors, caids, command)?;
        }

        if matched { Ok(Some(body)) } else { Ok(None) }
    }
}

fn parse_ca_descriptors(
    program_number: u16,
    descriptors: Option<DescriptorsRef<'_>>,
) -> Result<Vec<CaDescriptor>> {
    let mut result = Vec::new();
    let Some(descriptors) = descriptors else {
        return Ok(result);
    };

    for descriptor in descriptors {
        let descriptor = descriptor.map_err(|error| {
            Error::InvalidData(format!(
                "invalid descriptor in PMT program {program_number}: {error}"
            ))
        })?;
        if descriptor.tag() != CaDescriptorRef::TAG {
            continue;
        }

        let ca = CaDescriptorRef::try_from(descriptor).map_err(|error| {
            Error::InvalidData(format!(
                "invalid CA descriptor in PMT program {program_number}: {error}"
            ))
        })?;
        result.push(CaDescriptor {
            caid: ca.ca_system_id(),
            bytes: descriptor.bytes().to_vec(),
        });
    }

    Ok(result)
}

/// Writes program_info_length or ES_info_length, followed by the command and
/// the CA descriptors accepted by this CA resource session.
fn append_info(
    out: &mut Vec<u8>,
    descriptors: &[CaDescriptor],
    caids: &[u16],
    command: CaPmtCommand,
) -> Result<bool> {
    let matching: Vec<&CaDescriptor> = descriptors
        .iter()
        .filter(|descriptor| caids.contains(&descriptor.caid))
        .collect();
    if matching.is_empty() {
        out.extend_from_slice(&[0xF0, 0x00]);
        return Ok(false);
    }

    let descriptors_len: usize = matching
        .iter()
        .map(|descriptor| descriptor.bytes.len())
        .sum();
    let info_len = 1usize
        .checked_add(descriptors_len)
        .ok_or_else(|| Error::InvalidData("CA_PMT info length overflow".to_owned()))?;
    if info_len > 0x0FFF {
        return Err(Error::InvalidData(format!(
            "CA_PMT info is too large: {info_len} bytes"
        )));
    }

    let info_len = info_len as u16;
    out.extend_from_slice(&(0xF000 | info_len).to_be_bytes());
    out.push(command as u8);
    for descriptor in matching {
        out.extend_from_slice(&descriptor.bytes);
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use libmpegts::psi::{
        PmtBuilder,
        PmtConfig,
        PmtStream,
    };

    use super::*;

    fn ca_descriptor(caid: u16, pid: u16, private: &[u8]) -> Vec<u8> {
        let mut descriptor = vec![0x09, (4 + private.len()) as u8];
        descriptor.extend_from_slice(&caid.to_be_bytes());
        descriptor.extend_from_slice(&(0xE000 | pid).to_be_bytes());
        descriptor.extend_from_slice(private);
        descriptor
    }

    fn pmt_section() -> Vec<u8> {
        let mut program_descriptors = vec![0x52, 0x01, 0xAA];
        program_descriptors.extend(ca_descriptor(0x0100, 0x1ABC, &[0x55]));

        let mut video_descriptors = vec![0x52, 0x01, 0x01];
        video_descriptors.extend(ca_descriptor(0x0500, 0x0123, &[]));

        let sections = PmtBuilder::build(PmtConfig {
            program_number: 0x1234,
            pcr_pid: 0x0101,
            version: 3,
            program_descriptors,
            streams: vec![
                PmtStream {
                    stream_type: 0x1B,
                    elementary_pid: 0x0101,
                    stream_descriptors: video_descriptors,
                },
                PmtStream {
                    stream_type: 0x04,
                    elementary_pid: 0x0102,
                    stream_descriptors: Vec::new(),
                },
            ],
        });
        sections[0].to_vec()
    }

    #[test]
    fn builds_program_level_ca_pmt_and_keeps_all_streams() {
        let program = Program::parse(&pmt_section()).unwrap();
        let program_ca = ca_descriptor(0x0100, 0x1ABC, &[0x55]);

        let mut expected = vec![0x03, 0x12, 0x34, 0xC7, 0xF0, 0x08, 0x01];
        expected.extend_from_slice(&program_ca);
        expected.extend_from_slice(&[0x1B, 0xE1, 0x01, 0xF0, 0x00, 0x04, 0xE1, 0x02, 0xF0, 0x00]);

        assert_eq!(
            program
                .build_ca_pmt(
                    &[0x0100],
                    CaPmtListManagement::Only,
                    CaPmtCommand::OkDescrambling,
                )
                .unwrap(),
            Some(expected)
        );
    }

    #[test]
    fn filters_ca_descriptors_for_each_session() {
        let program = Program::parse(&pmt_section()).unwrap();
        let video_ca = ca_descriptor(0x0500, 0x0123, &[]);

        let mut expected = vec![
            0x04, 0x12, 0x34, 0xC7, 0xF0, 0x00, 0x1B, 0xE1, 0x01, 0xF0, 0x07, 0x01,
        ];
        expected.extend_from_slice(&video_ca);
        expected.extend_from_slice(&[0x04, 0xE1, 0x02, 0xF0, 0x00]);

        assert_eq!(
            program
                .build_ca_pmt(
                    &[0x0500],
                    CaPmtListManagement::Add,
                    CaPmtCommand::OkDescrambling,
                )
                .unwrap(),
            Some(expected)
        );
        assert_eq!(
            program
                .build_ca_pmt(
                    &[0x0600],
                    CaPmtListManagement::Only,
                    CaPmtCommand::OkDescrambling,
                )
                .unwrap(),
            None
        );
    }

    #[test]
    fn builds_not_selected_command() {
        let program = Program::parse(&pmt_section()).unwrap();
        let body = program
            .build_ca_pmt(
                &[0x0100],
                CaPmtListManagement::Update,
                CaPmtCommand::NotSelected,
            )
            .unwrap()
            .unwrap();

        assert_eq!(body[0], CaPmtListManagement::Update as u8);
        assert_eq!(body[6], CaPmtCommand::NotSelected as u8);
    }

    #[test]
    fn rejects_invalid_section_and_ca_descriptor() {
        let mut bad_crc = pmt_section();
        bad_crc[3] ^= 1;
        assert!(Program::parse(&bad_crc).is_err());

        let sections = PmtBuilder::build(PmtConfig {
            program_number: 1,
            pcr_pid: 0x0100,
            version: 0,
            program_descriptors: vec![0x09, 0x03, 0x01, 0x00, 0xE1],
            streams: Vec::new(),
        });
        assert!(Program::parse(&sections[0]).is_err());
    }
}
