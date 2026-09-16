//! Field positions, and the check that the fields cover every bit.

use super::{Body, Item, Plan};

impl Plan<'_> {
    pub(super) fn solve(&mut self) -> syn::Result<()> {
        let total = u64::from(self.bits);
        let (unit, noun) = self.unit();
        let mut at = u64::from(self.prefix);
        let mut items = core::mem::take(&mut self.items);

        for item in &mut items {
            self.check_placement(item, at)?;

            let end = at + item.body.width();
            if end > total {
                return Err(syn::Error::new(
                    item.span,
                    format!(
                        "field `{}`: {noun}s {}..{} overflow the {}",
                        item.ident,
                        at / unit,
                        end / unit,
                        self.capacity(),
                    ),
                ));
            }

            let kind = item.body.kind();
            item.phys_bit = self.grid.physical(&kind, at);
            if let Some(assertion) = item.at {
                self.grid
                    .check_stated(&item.ident.to_string(), &kind, assertion.stated, at)
                    .map_err(|why| syn::Error::new(assertion.span, why))?;
            }
            at = end;
        }

        self.items = items;
        if at != total {
            return Err(syn::Error::new(self.ident.span(), self.shortfall(at, total)));
        }
        Ok(())
    }

    fn check_placement(&self, item: &Item, at: u64) -> syn::Result<()> {
        let Some(region) = self.interior_boundary() else {
            return Ok(());
        };
        match &item.body {
            Body::Packed(p) => {
                let width = u64::from(p.bits);
                if at % region + width > region {
                    return Err(syn::Error::new(
                        p.bits_span,
                        format!(
                            "field `{}`: bits {at}..{} cross a {region}-bit word boundary. \
                             Fix the widths before it, or add a spare field",
                            item.ident,
                            at + width,
                        ),
                    ));
                }
            }
            body => {
                let align = region.min(body.element_bits());
                if align != 0 && !at.is_multiple_of(align) {
                    let adverb = if align == region { "word-aligned" } else { "byte-aligned" };
                    let span = match body {
                        Body::Nested { span, .. } => *span,
                        _ => item.span,
                    };
                    return Err(syn::Error::new(
                        span,
                        format!(
                            "field `{}`: {} must be {adverb}, but starts at bit {at}",
                            item.ident,
                            body.describe(),
                        ),
                    ));
                }
            }
        }
        Ok(())
    }

    fn capacity(&self) -> String {
        format!("{}-{} layout", self.grid.units(), self.grid.unit().keyword().trim_end_matches('s'))
    }

    fn shortfall(&self, at: u64, total: u64) -> String {
        let (unit, noun) = self.unit();
        let claim = if self.prefix == 0 {
            format!("declares {} {}", self.grid.units(), self.grid.unit().keyword())
        } else {
            format!(
                "declares {} {} ({} after the {}-bit prefix)",
                self.grid.units(),
                self.grid.unit().keyword(),
                self.bits - self.prefix,
                self.prefix,
            )
        };
        let reached = at / unit;
        let after = self.items.last().map(|i| format!(" after `{}`", i.ident)).unwrap_or_default();
        let fix = if self.grid.is_bit_addressed() {
            "named spare fields"
        } else {
            "reserved `[u8; N]` fields"
        };
        format!(
            "`{}`: {claim} but its fields end at {noun} {reached}. \
             Add {} {noun}s of {fix}{after}",
            self.ident,
            (total - at) / unit,
        )
    }
}
