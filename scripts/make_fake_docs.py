#!/usr/bin/env python3
"""Make fake private paperwork as PDFs, to test shoebox without real documents.

All persons, companies, numbers and amounts are invented. IBANs use the
invalid check digits "00", so no IBAN can belong to a real account.

Usage:
    python3 scripts/make_fake_docs.py [OUT_DIR]     (default: testdata/fake)

Writes the PDFs (ignored by git) and expected.tsv (the correct answers).
Plain Python, no extra packages. The PDFs have a text layer, so no OCR is needed.
"""

import sys
from pathlib import Path

# ---------------------------------------------------------------- PDF writer

PAGE_W, PAGE_H = 595, 842  # A4 in points
LEFT, TOP, BOTTOM = 70, 780, 60


def esc(text):
    return (
        text.replace("\\", "\\\\")
        .replace("(", "\\(")
        .replace(")", "\\)")
        .encode("cp1252")
    )


def page_stream(lines, footer):
    """lines: list of (text, size, bold). Empty text = vertical space."""
    out = [b"BT"]
    y = TOP
    for text, size, bold in lines:
        if text:
            font = b"/F2" if bold else b"/F1"
            out.append(b"%s %d Tf 1 0 0 1 %d %d Tm (%s) Tj" % (font, size, LEFT, y, esc(text)))
        y -= int(size * 1.5)
        if y < BOTTOM + 20:
            raise ValueError("page too long: " + repr(lines[0][0]))
    if footer:
        out.append(b"/F1 9 Tf 1 0 0 1 %d %d Tm (%s) Tj" % (PAGE_W - 150, BOTTOM - 20, esc(footer)))
    out.append(b"ET")
    return b"\n".join(out)


def write_pdf(path, pages):
    """pages: list of (lines, footer). A page with no lines is a blank page."""
    objs = {}
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[3] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
    objs[4] = b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
    kids = []
    n = 5
    for lines, footer in pages:
        content = page_stream(lines, footer) if (lines or footer) else b""
        objs[n] = b"<< /Length %d >>\nstream\n%s\nendstream" % (len(content), content)
        objs[n + 1] = (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %d %d] "
            b"/Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents %d 0 R >>"
            % (PAGE_W, PAGE_H, n)
        )
        kids.append(b"%d 0 R" % (n + 1))
        n += 2
    objs[2] = b"<< /Type /Pages /Kids [%s] /Count %d >>" % (b" ".join(kids), len(kids))

    data = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offsets = {}
    for i in sorted(objs):
        offsets[i] = len(data)
        data += b"%d 0 obj\n%s\nendobj\n" % (i, objs[i])
    xref = len(data)
    data += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for i in sorted(objs):
        data += b"%010d 00000 n \n" % offsets[i]
    data += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
    path.write_bytes(bytes(data))


# ---------------------------------------------------------------- text helpers


def H(text):
    return (text, 14, True)


def B(text):
    return (text, 10, True)


def T(text):
    return (text, 10, False)


SPACE = ("", 10, False)

ME = ["Erika Mustermann", "Lindenweg 12", "34567 Beispielstadt"]


def letter(sender, recipient, place_date, subject, refs):
    lines = [T(sender), SPACE]
    lines += [T(x) for x in recipient]
    lines += [SPACE, T(place_date), SPACE]
    lines += [T(r) for r in refs]
    lines += [SPACE, H(subject), SPACE]
    return lines


def numbered(pages, style="Seite {n} von {m}", header=None):
    """Adds the printed page number. Like real letters, pages after the first
    repeat the references in a running header (counterpart matching needs them)."""
    m = len(pages)
    out = []
    for i, lines in enumerate(pages):
        if header and i > 0:
            lines = [T(header), SPACE] + lines
        out.append((lines, style.format(n=i + 1, m=m)))
    return out


# ---------------------------------------------------------------- documents


def mietvertrag():
    p1 = [
        H("Wohnraummietvertrag"),
        T("Vertragsnummer: MV-2024-0381"),
        SPACE,
        B("Zwischen"),
        T("Hausverwaltung Sonnenhof GmbH, Am Markt 3, 34567 Beispielstadt"),
        T("- nachfolgend Vermieter -"),
        B("und"),
        T("Erika Mustermann, bisher wohnhaft Kastanienallee 7, 34560 Altdorf"),
        T("- nachfolgend Mieter -"),
        T("wird folgender Mietvertrag geschlossen:"),
        SPACE,
        B("§ 1 Mietsache"),
        T("Vermietet wird die Wohnung Lindenweg 12, 2. OG links, 34567 Beispielstadt."),
        T("Die Wohnung besteht aus 3 Zimmern, Küche, Bad, Flur und Balkon."),
        T("Die Wohnfläche beträgt 74,5 m². Zur Wohnung gehört der Kellerraum Nr. 6."),
        SPACE,
        B("§ 2 Mietzeit"),
        T("Das Mietverhältnis beginnt am 01.04.2024 und läuft auf unbestimmte Zeit."),
        T("Die gesetzlichen Kündigungsfristen gelten (§ 573c BGB)."),
    ]
    p2 = [
        B("§ 3 Miete und Nebenkosten"),
        T("Die monatliche Grundmiete beträgt 780,00 EUR."),
        T("Die Vorauszahlung auf die Betriebskosten beträgt 165,00 EUR."),
        T("Die Vorauszahlung auf die Heizkosten beträgt 85,00 EUR."),
        T("Gesamtmiete monatlich: 1.030,00 EUR."),
        SPACE,
        B("§ 4 Zahlung"),
        T("Die Miete ist monatlich im Voraus, spätestens am 3. Werktag, zu zahlen auf:"),
        T("IBAN DE00 3456 7000 0012 3456 78, Sparkasse Beispielstadt"),
        T("Verwendungszweck: MV-2024-0381"),
        SPACE,
        B("§ 5 Mietsicherheit"),
        T("Der Mieter leistet eine Kaution von 2.340,00 EUR (drei Grundmieten)."),
        T("Er darf die Kaution in drei gleichen monatlichen Teilzahlungen leisten."),
    ]
    p3 = [
        B("§ 6 Schönheitsreparaturen"),
        T("Die Wohnung wird renoviert übergeben. Schönheitsreparaturen trägt der Mieter."),
        SPACE,
        B("§ 7 Tierhaltung"),
        T("Kleintiere sind erlaubt. Für Hunde und Katzen ist die Zustimmung nötig."),
        SPACE,
        B("§ 8 Hausordnung"),
        T("Die Hausordnung in der Fassung vom 01.01.2023 ist Bestandteil des Vertrags."),
    ]
    p4 = [
        B("§ 9 Sonstige Vereinbarungen"),
        T("Der Mieter erhält 3 Haustürschlüssel und 2 Briefkastenschlüssel."),
        SPACE,
        T("Beispielstadt, den 12.02.2024"),
        SPACE,
        SPACE,
        T("______________________________        ______________________________"),
        T("Vermieter                                                Mieter"),
        T("Hausverwaltung Sonnenhof GmbH                  Erika Mustermann"),
    ]
    return numbered([p1, p2, p3, p4], header="Mietvertrag MV-2024-0381 vom 12.02.2024")


def steuerbescheid():
    p1 = letter(
        "Finanzamt Beispielstadt, Postfach 10 20 30, 34501 Beispielstadt",
        ME,
        "Beispielstadt, 14.07.2026",
        "Bescheid für 2025 über Einkommensteuer",
        ["Steuernummer: 012/345/67890", "Identifikationsnummer: 00 123 456 789"],
    ) + [
        T("Der Bescheid ergeht aufgrund Ihrer Erklärung vom 28.04.2026."),
        SPACE,
        B("Festsetzung"),
        T("Einkommensteuer                                     6.412,00 EUR"),
        T("Solidaritätszuschlag                                          0,00 EUR"),
        T("Kirchensteuer                                                   0,00 EUR"),
        SPACE,
        B("Abrechnung"),
        T("Festgesetzt                                              6.412,00 EUR"),
        T("Ab: Steuerabzug vom Lohn                       7.287,00 EUR"),
        T("Erstattung                                                 875,00 EUR"),
        SPACE,
        T("Der Betrag von 875,00 EUR wird auf Ihr Konto DE00 5001 0517 0000 4711 11 erstattet."),
    ]
    p2 = [
        B("Ermittlung des zu versteuernden Einkommens"),
        T("Einkünfte aus nichtselbständiger Arbeit"),
        T("   Bruttoarbeitslohn                                  48.600,00 EUR"),
        T("   Werbungskosten                                      -2.130,00 EUR"),
        T("Summe der Einkünfte                                46.470,00 EUR"),
        T("Sonderausgaben                                        -6.025,00 EUR"),
        T("Außergewöhnliche Belastungen                          0,00 EUR"),
        T("Zu versteuerndes Einkommen                     40.445,00 EUR"),
        SPACE,
        B("Erläuterungen"),
        T("Die Fahrtkosten wurden für 210 Arbeitstage anerkannt."),
        T("Die Aufwendungen für das Arbeitszimmer sind nicht abziehbar."),
    ]
    p3 = [
        B("Rechtsbehelfsbelehrung"),
        T("Sie können gegen diesen Bescheid Einspruch einlegen."),
        T("Die Frist beträgt einen Monat nach Bekanntgabe des Bescheids."),
        T("Der Einspruch ist beim Finanzamt Beispielstadt einzulegen."),
        SPACE,
        T("Steuernummer: 012/345/67890"),
    ]
    return numbered([p1, p2, p3], header="Steuernummer 012/345/67890, Bescheid vom 14.07.2026")


def stromrechnung():
    p1 = letter(
        "Stadtwerke Beispielstadt GmbH, Werkstraße 1, 34567 Beispielstadt",
        ME,
        "Beispielstadt, 20.01.2026",
        "Jahresabrechnung Strom 2025",
        ["Kundennummer: 700 123 456", "Vertragskonto: 2100.4455.01", "Rechnungsnummer: R-2026-018832"],
    ) + [
        T("Abrechnungszeitraum: 01.01.2025 bis 31.12.2025"),
        T("Zähler Nr. 1EMH0012345678, Lindenweg 12"),
        T("Zählerstand alt: 41.208 kWh, Zählerstand neu: 43.519 kWh"),
        T("Verbrauch: 2.311 kWh"),
        SPACE,
        T("Arbeitspreis 2.311 kWh x 0,3412 EUR                 788,51 EUR"),
        T("Grundpreis 12 Monate x 12,90 EUR                     154,80 EUR"),
        T("Summe brutto                                                943,31 EUR"),
        T("Geleistete Abschläge 12 x 82,00 EUR                984,00 EUR"),
        B("Guthaben                                                         40,69 EUR"),
    ]
    p2 = [
        B("Neuer Abschlag"),
        T("Ab 01.02.2026 beträgt Ihr monatlicher Abschlag 79,00 EUR."),
        T("Wir buchen den Betrag per SEPA-Lastschrift ab."),
        T("Mandatsreferenz: SWB-700123456"),
        SPACE,
        T("Das Guthaben von 40,69 EUR überweisen wir in den nächsten Tagen."),
    ]
    return numbered([p1, p2], header="Kundennummer 700 123 456, Rechnung R-2026-018832 vom 20.01.2026")


def kfz_versicherung():
    p1 = letter(
        "Nordlicht Versicherung AG, Hafenstraße 99, 20000 Musterhafen",
        ME,
        "Musterhafen, 02.12.2025",
        "Versicherungsschein Kfz-Versicherung",
        ["Versicherungsnummer: KV-55.123.987", "Kennzeichen: BS-EM 123"],
    ) + [
        T("Fahrzeug: Kleinwagen, Erstzulassung 05/2019, FIN WVWZZZ0000000000"),
        T("Versicherungsbeginn: 01.01.2026, Hauptfälligkeit: 01.01."),
        SPACE,
        T("Kfz-Haftpflicht, SF-Klasse 12                            312,40 EUR"),
        T("Teilkasko, Selbstbeteiligung 150 EUR                 98,60 EUR"),
        T("Jahresbeitrag inkl. 19 % Versicherungsteuer      411,00 EUR"),
    ]
    p2 = [
        B("Zahlung"),
        T("Der Jahresbeitrag wird am 01.01.2026 per Lastschrift eingezogen."),
        SPACE,
        B("Hinweis"),
        T("Bitte melden Sie Schäden innerhalb einer Woche."),
        T("Versicherungsnummer: KV-55.123.987"),
    ]
    return numbered([p1, p2], header="Versicherungsnummer KV-55.123.987 vom 02.12.2025")


def arbeitsvertrag():
    p1 = [
        H("Arbeitsvertrag"),
        T("zwischen der Feinmechanik Huber KG, Industriering 8, 34567 Beispielstadt"),
        T("und Frau Erika Mustermann, Lindenweg 12, 34567 Beispielstadt"),
        T("Personalnummer: 10482"),
        SPACE,
        B("§ 1 Beginn und Tätigkeit"),
        T("Das Arbeitsverhältnis beginnt am 01.03.2025."),
        T("Frau Mustermann wird als Sachbearbeiterin Einkauf eingestellt."),
        SPACE,
        B("§ 2 Probezeit"),
        T("Die ersten sechs Monate gelten als Probezeit."),
    ]
    p2 = [
        B("§ 3 Arbeitszeit"),
        T("Die regelmäßige wöchentliche Arbeitszeit beträgt 38,5 Stunden."),
        SPACE,
        B("§ 4 Vergütung"),
        T("Das monatliche Bruttogehalt beträgt 4.050,00 EUR."),
        T("Es wird jeweils zum Monatsende gezahlt."),
        SPACE,
        B("§ 5 Urlaub"),
        T("Der Urlaubsanspruch beträgt 30 Arbeitstage im Kalenderjahr."),
    ]
    p3 = [
        B("§ 6 Kündigung"),
        T("Nach der Probezeit gelten die gesetzlichen Kündigungsfristen."),
        SPACE,
        T("Beispielstadt, 15.01.2025"),
        SPACE,
        T("______________________________        ______________________________"),
        T("Feinmechanik Huber KG                          Erika Mustermann"),
    ]
    return numbered([p1, p2, p3], header="Arbeitsvertrag Personalnummer 10482 vom 15.01.2025")


def nebenkosten(year, date, invoice, amount, extra):
    """A 4-page service charge statement from the landlord (same contract each year)."""
    pages = [
        letter(
            "Hausverwaltung Sonnenhof GmbH, Am Markt 3, 34567 Beispielstadt",
            ME,
            "Beispielstadt, " + date,
            f"Betriebskostenabrechnung {year}",
            ["Mietvertrag: MV-2024-0381", "Objekt: Lindenweg 12, 2. OG links", f"Abrechnung Nr. {invoice}"],
        )
        + [
            T(f"Abrechnungszeitraum: 01.01.{year} bis 31.12.{year}"),
            T("Ihr Anteil nach Wohnfläche: 74,5 m² von 612,0 m²"),
            SPACE,
            B(f"Ergebnis: {amount}"),
            T("Die Einzelheiten finden Sie auf den folgenden Seiten."),
        ],
        [
            B("Kalte Betriebskosten"),
            T("Grundsteuer                                              312,10 EUR"),
            T("Wasser und Abwasser                                 401,55 EUR"),
            T("Müllabfuhr                                               188,20 EUR"),
            T("Hausreinigung                                          240,00 EUR"),
            T("Gartenpflege                                              96,30 EUR"),
            T("Gebäudeversicherung                                   154,85 EUR"),
        ],
        [
            B("Heizkosten nach HeizkostenV"),
            T("Grundkosten 30 % nach Fläche                         298,40 EUR"),
            T("Verbrauchskosten 70 % nach Zählern                  712,60 EUR"),
            T(extra),
        ],
        [
            B("Zahlung"),
            T("Bitte überweisen Sie den Betrag bis zum 30. Tag nach Zugang dieses Schreibens."),
            T("IBAN DE00 3456 7000 0012 3456 78, Verwendungszweck: " + invoice),
            SPACE,
            T("Sie können die Belege bei uns nach Terminabsprache einsehen."),
        ],
    ]
    return numbered(pages, header=f"Mietvertrag MV-2024-0381, Abrechnung {invoice} vom {date}")


def kontoauszug():
    pages = [
        [
            H("Kontoauszug 11/2025"),
            T("Sparkasse Beispielstadt"),
            T("Konto DE00 5001 0517 0000 4711 11, Erika Mustermann"),
            T("Auszug Nr. 11, 01.11.2025 bis 30.11.2025"),
            SPACE,
            T("Alter Kontostand                                         2.140,17 EUR"),
            T("03.11. Hausverwaltung Sonnenhof MV-2024-0381     -1.030,00 EUR"),
            T("05.11. Stadtwerke Beispielstadt Abschlag                  -82,00 EUR"),
            T("12.11. Supermarkt Frisch                                      -64,31 EUR"),
            T("28.11. Gehalt Feinmechanik Huber KG                  +2.611,54 EUR"),
        ],
        [
            T("29.11. Kontoführung                                               -4,90 EUR"),
            T("Neuer Kontostand                                         3.570,50 EUR"),
        ],
    ]
    return numbered(pages, "Blatt {n} aus {m}", header="Konto DE00 5001 0517 0000 4711 11, Auszug 11/2025")


# ---------------------------------------------------------------- scan passes


def blank():
    return ([], None)


def build():
    """Returns {filename: pages} and the expected answers."""
    docs, expected = {}, []

    def add(name, pages, kind, date, check, counterpart="", note=""):
        docs[name] = pages
        expected.append((name, kind, date, len(pages), check, counterpart, note))

    add("mietvertrag.pdf", mietvertrag(), "Mietvertrag", "2024-02-12", "complete")
    add("steuerbescheid_2025.pdf", steuerbescheid(), "Einkommensteuerbescheid", "2026-07-14", "complete")
    add("stromrechnung_2025.pdf", stromrechnung(), "Stromrechnung", "2026-01-20", "complete")
    add("kfz_versicherungsschein.pdf", kfz_versicherung(), "Versicherungsschein", "2025-12-02", "complete")
    add("kontoauszug_2025-11.pdf", kontoauszug(), "Kontoauszug", "2025-11-30", "complete",
        note="page numbers as 'Blatt N aus M'")

    # Duplex scan in two passes: fronts (1, 3), then backs in reverse order (4, 2).
    nk = nebenkosten("2025", "18.03.2026", "BK-2025-0381", "Nachzahlung 127,45 EUR",
                     "Warmwasser                                               164,80 EUR")
    add("scan_0001.pdf", [nk[0], nk[2]], "Betriebskostenabrechnung", "2026-03-18",
        "incomplete: 2, 4 missing", "scan_0002.pdf", "front pass of a duplex scan")
    add("scan_0002.pdf", [nk[3], nk[1]], "Betriebskostenabrechnung", "2026-03-18",
        "incomplete: 1, 3 missing", "scan_0001.pdf", "back pass, reverse order")

    # Distractor: the same landlord and contract one year earlier, only pages 2 and 4.
    # It shares the contract number, but not the statement number or the date.
    old = nebenkosten("2024", "21.03.2025", "BK-2024-0381", "Guthaben 52,10 EUR",
                      "Warmwasser                                               149,20 EUR")
    add("scan_0003.pdf", [old[1], old[3]], "Betriebskostenabrechnung", "2025-03-21",
        "incomplete: 1, 3 missing", "",
        "distractor: must NOT match scan_0001; its pages 1 and 3 are not in the pile")

    # Duplex back pass with a blank page (the back of a one-sided page).
    av = arbeitsvertrag()
    add("scan_0004.pdf", [av[0], av[2]], "Arbeitsvertrag", "2025-01-15",
        "incomplete: 2 missing", "scan_0005.pdf", "front pass")
    add("scan_0005.pdf", [blank(), av[1]], "Arbeitsvertrag", "2025-01-15",
        "incomplete: 1, 3 missing", "scan_0004.pdf", "back pass; first page is blank")

    return docs, expected


def main():
    root = Path(__file__).resolve().parent.parent
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "testdata" / "fake"
    out.mkdir(parents=True, exist_ok=True)
    docs, expected = build()
    for name, pages in docs.items():
        write_pdf(out / name, pages)
    header = ("file", "kind", "date", "pages", "check", "counterpart", "note")
    rows = ["\t".join(header)] + ["\t".join(str(x) for x in row) for row in expected]
    (out / "expected.tsv").write_text("\n".join(rows) + "\n", encoding="utf-8")
    print(f"wrote {len(docs)} PDFs and expected.tsv to {out}")


if __name__ == "__main__":
    main()
