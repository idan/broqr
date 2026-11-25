use serde::Serialize;

use crate::{Error, Result};

/// Only documented fields are interpreted; reserved bytes vary by firmware.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrinterStatus {
    pub model_code: u8,
    pub errors: u16,
    pub media_width_mm: u8,
    pub media_type: u8,
    pub media_length_mm: u8,
    pub status_type: u8,
    pub phase: u8,
}

impl PrinterStatus {
    pub(crate) fn parse(packet: &[u8; 32]) -> Result<Self> {
        if packet[..4] != [0x80, 0x20, 0x42, 0x34] {
            return Err(Error::Protocol("Not a Brother QL status packet.".into()));
        }
        Ok(Self {
            model_code: packet[4],
            errors: u16::from_le_bytes([packet[8], packet[9]]),
            media_width_mm: packet[10],
            media_type: packet[11],
            media_length_mm: packet[17],
            status_type: packet[18],
            phase: packet[19],
        })
    }

    pub(crate) fn check_errors(&self) -> Result<()> {
        if self.errors != 0 {
            let names = [
                (0x0001, "no tape loaded"),
                (0x0002, "end of tape"),
                (0x0004, "cutter jam"),
                (0x0010, "printer is in use"),
                (0x0020, "printer was turned off"),
                (0x0100, "replace tape / media mismatch"),
                (0x0200, "expansion buffer full"),
                (0x0400, "communication error"),
                (0x1000, "cover is open"),
                (0x4000, "tape cannot be fed or has run out"),
                (0x8000, "system error"),
            ];
            let mut messages: Vec<_> = names
                .iter()
                .filter(|(mask, _)| self.errors & mask != 0)
                .map(|(_, name)| *name)
                .collect();
            if messages.is_empty() {
                messages.push("unrecognized error");
            }
            return Err(Error::Printer(format!(
                "{} (error flags 0x{:04x}).",
                messages.join(", "),
                self.errors
            )));
        }
        if self.status_type == 0x02 || self.status_type == 0x04 {
            return Err(Error::Printer(
                "The printer reported an error or was turned off.".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn check_ready(&self) -> Result<()> {
        self.check_errors()?;
        if self.model_code != 0x39 {
            return Err(Error::Printer(format!(
                "This app targets the QL-810W; the connected device reported model 0x{:02x}.",
                self.model_code
            )));
        }
        // The official status value is 4Ah. Some firmware/libraries report
        // 0Ah, the equivalent media identifier used in print commands.
        if self.media_width_mm != 62
            || !matches!(self.media_type, 0x4a | 0x0a)
            || self.media_length_mm != 0
        {
            return Err(Error::Printer(format!(
                "Load 62 mm continuous black-on-white tape. Reported media: {} mm wide,                  type 0x{:02x}, length {} mm.",
                self.media_width_mm, self.media_type, self.media_length_mm
            )));
        }
        if self.phase != 0 {
            return Err(Error::Printer(
                "The printer is already printing. Wait and try again.".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn packet(status_type: u8) -> [u8; 32] {
        let mut p = [0; 32];
        p[..4].copy_from_slice(&[0x80, 0x20, 0x42, 0x34]);
        p[4] = 0x39;
        p[10] = 62;
        p[11] = 0x4a;
        p[18] = status_type;
        p
    }

    #[test]
    fn accepts_documented_media_and_variable_reserved_bytes() {
        let mut p = packet(0);
        p[6] = 4;
        p[14] = 0x15;
        PrinterStatus::parse(&p).unwrap().check_ready().unwrap();
        p[11] = 0x0a;
        PrinterStatus::parse(&p).unwrap().check_ready().unwrap();
    }

    #[test]
    fn rejects_wrong_model_wrong_media_and_busy_printer() {
        for (index, value) in [(4, 0x41), (10, 29), (11, 0x4b), (17, 100), (19, 1)] {
            let mut p = packet(0);
            p[index] = value;
            assert!(PrinterStatus::parse(&p).unwrap().check_ready().is_err());
        }
    }

    #[test]
    fn decodes_error_bits_and_rejects_non_brother_packet() {
        let mut p = packet(2);
        p[9] = 0x10;
        let error = PrinterStatus::parse(&p)
            .unwrap()
            .check_ready()
            .unwrap_err()
            .to_string();
        assert!(error.contains("cover is open"));
        assert!(PrinterStatus::parse(&[0; 32]).is_err());
    }
}
