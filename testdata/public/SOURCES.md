# Public test documents

Public sample PDFs to test `shoebox` without private paperwork.
Downloaded on 2026-11-11.

The PDFs are **not in git**: the root `.gitignore` blocks all `*.pdf` files.
Only this file is committed. To get the PDFs again, use the URLs below.

None of these files contain data about real private persons.

## invoices/ — German e-invoice samples (ZUGFeRD 2.5.2 / Factur-X)

Owner: FeRD (Forum elektronische Rechnung Deutschland), official ZUGFeRD 2.5.2 sample package.
Copy taken from the Mustang project, which ships the samples as test resources:
<https://github.com/ZUGFeRD/mustangproject> (Apache-2.0), folder
`library/src/test/resources/ZF252_Beispiele/`.
Companies, persons and amounts in the samples are fictitious.

| Local file | Original path (in `ZF252_Beispiele/`) | Content |
| --- | --- | --- |
| `B02_Rechnungskorrektur.pdf` | `2. BASIC/B02_Rechnungskorrektur/B02_01_Rechnungskorrektur_fx.pdf` | Corrected invoice |
| `B03_Taxifahrt.pdf` | `2. BASIC/B03_Taxifahrt/B03_01_Taxifahrt_fx.pdf` | Taxi receipt |
| `E04_Betriebskostenabrechnung.pdf` | `3. EN16931/E04_Betriebskostenabrechnung_NEU/E04_01_Betriebskostenabrechnung_NEU_fx.pdf` | Service charge statement (rent) |
| `E05_Einfach.pdf` | `3. EN16931/E05_Einfach/E05_01_Einfach_fx.pdf` | Simple invoice |
| `E10_Gutschrift.pdf` | `3. EN16931/E10_Gutschrift/E10_01_Gutschrift_fx.pdf` | Credit note |
| `E11_Haftpflichtversicherung.pdf` | `3. EN16931/E11_Haftpflichtversicherung_Versicherungssteuer/E11_01_Haftpflichtversicherung_Versicherungssteuer_fx.pdf` | Liability insurance invoice |
| `E14_Kraftfahrversicherung.pdf` | `3. EN16931/E14_Kraftfahrversicherung/E14_01_Kraftfahrversicherung_fx.pdf` | Car insurance invoice |
| `E15_Miete.pdf` | `3. EN16931/E15_Miete/E15_01_Miete_fx.pdf` | Rent invoice |

Raw URL pattern:
`https://raw.githubusercontent.com/ZUGFeRD/mustangproject/master/library/src/test/resources/ZF252_Beispiele/<path>`

## forms/ — US tax forms (blank)

Owner: US Internal Revenue Service. US government works, public domain.

| Local file | URL | Content |
| --- | --- | --- |
| `irs_f1040.pdf` | <https://www.irs.gov/pub/irs-pdf/f1040.pdf> | Form 1040, income tax return |
| `irs_fw9.pdf` | <https://www.irs.gov/pub/irs-pdf/fw9.pdf> | Form W-9 |
| `irs_f1099nec.pdf` | <https://www.irs.gov/pub/irs-pdf/f1099nec.pdf> | Form 1099-NEC |

## laws/ — German regulations

Owner: Bundesministerium der Justiz, via <https://www.gesetze-im-internet.de>.
Official works, no copyright (§ 5 UrhG).

| Local file | URL | Content |
| --- | --- | --- |
| `BetrKV.pdf` | <https://www.gesetze-im-internet.de/betrkv/BetrKV.pdf> | Betriebskostenverordnung |
| `HeizkostenV.pdf` | <https://www.gesetze-im-internet.de/heizkostenv/HeizkostenV.pdf> | Heizkostenverordnung |

## SHA-256 (first 12 characters)

```
3d31c226df0d forms/irs_f1040.pdf
68a8f00078be forms/irs_f1099nec.pdf
2d420cbb4123 forms/irs_fw9.pdf
df1e0e1e8222 invoices/B02_Rechnungskorrektur.pdf
9a219db79338 invoices/B03_Taxifahrt.pdf
585e7ff54dbd invoices/E04_Betriebskostenabrechnung.pdf
a0978983423b invoices/E05_Einfach.pdf
e8d86c3070b4 invoices/E10_Gutschrift.pdf
1cc8f281c4e1 invoices/E11_Haftpflichtversicherung.pdf
a2233e5040bb invoices/E14_Kraftfahrversicherung.pdf
adcf13f4157b invoices/E15_Miete.pdf
db615c6e06c0 laws/BetrKV.pdf
5242637d8d16 laws/HeizkostenV.pdf
```
