//! Donation support: a UPI ID + an offline-generated QR (any-amount).
//!
//! Fully offline — the QR is rendered to inline SVG from the static UPI payment
//! string; no network, no third-party service. Surfaced by `get_donation_info`
//! and shown in the Settings "Support Nirvana" section.

use crate::error::{CoreError, CoreResult};
use qrcode::render::svg;
use qrcode::QrCode;
use serde::Serialize;

/// The payee VPA (UPI ID). Public, not a secret.
pub const UPI_ID: &str = "deveshwar.jaiswal996@okhdfcbank";
const PAYEE_NAME: &str = "Deveshwar Jaiswal";

/// The standard UPI deep-link the QR encodes. No `am` (amount) → the payer
/// chooses any amount in their UPI app. `cu=INR` sets the currency.
pub fn upi_uri() -> String {
    format!(
        "upi://pay?pa={UPI_ID}&pn={}&cu=INR",
        PAYEE_NAME.replace(' ', "%20")
    )
}

/// What the UI needs to render the donation section.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DonationInfo {
    /// The UPI ID, shown as copyable text.
    pub upi_id: String,
    /// A self-contained SVG QR of [`upi_uri`] (black on white for scannability).
    pub qr_svg: String,
}

/// Build the donation info, rendering the UPI QR to SVG.
pub fn donation_info() -> CoreResult<DonationInfo> {
    let code = QrCode::new(upi_uri().as_bytes())
        .map_err(|e| CoreError::Unsupported(format!("qr: {e}")))?;
    let qr_svg = code
        .render::<svg::Color>()
        .min_dimensions(220, 220)
        .dark_color(svg::Color("#000000"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(DonationInfo {
        upi_id: UPI_ID.to_string(),
        qr_svg,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upi_uri_carries_id_currency_and_encodes_name() {
        let uri = upi_uri();
        assert!(uri.starts_with("upi://pay?"));
        assert!(uri.contains(&format!("pa={UPI_ID}")));
        assert!(uri.contains("cu=INR"));
        assert!(uri.contains("pn=Deveshwar%20Jaiswal"));
    }

    #[test]
    fn donation_info_renders_an_svg_qr() {
        let info = donation_info().unwrap();
        assert_eq!(info.upi_id, UPI_ID);
        assert!(info.qr_svg.contains("<svg"));
        assert!(info.qr_svg.contains("</svg>"));
        assert!(info.qr_svg.len() > 200, "QR svg should be substantial");
    }
}
