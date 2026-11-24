//! Sample workbooks: the golden test, the Excel/LibreOffice open check (`examples/samples.rs`)
//! and the memory gate build them from here.
#![allow(dead_code, clippy::unwrap_used)]

use uo_xlsx::{
    BorderEdge, BorderStyle, Borders, Cell, CellRange, CellValue, Column, DateTime, DefinedName, HAlign, Link, Rgb, RowOptions,
    SheetOptions, Style, StyleId, WorkbookOptions, XlsxWriter,
};

pub fn workbook_options() -> WorkbookOptions {
    WorkbookOptions {
        title: Some("Relatório de conformidade".into()),
        creator: Some("uo-xlsx tests".into()),
        language: Some("pt-BR".into()),
        created: Some("2026-11-23T12:00:00Z".into()),
        ..WorkbookOptions::default()
    }
}

/// One cell.
pub fn minimal() -> Vec<u8> {
    let mut w = XlsxWriter::new(WorkbookOptions::default()).unwrap();
    w.begin_sheet(SheetOptions { name: "Data".into(), ..SheetOptions::default() }).unwrap();
    w.append_row(&RowOptions::default(), &[Cell::text("hello")]).unwrap();
    w.finish().unwrap()
}

/// Every feature of the writer on two sheets (the second right-to-left).
pub fn features(shared_strings: bool) -> Vec<u8> {
    let mut w = XlsxWriter::new(WorkbookOptions { shared_strings, ..workbook_options() }).unwrap();
    let thin = BorderEdge { style: BorderStyle::Thin, color: Some(Rgb(0x808080)) };
    let header = w
        .add_style(&Style {
            bold: true,
            fill: Some(Rgb(0x1F4E79)),
            color: Some(Rgb(0xFFFFFF)),
            border: Borders::all(thin),
            align: Some(HAlign::Center),
            wrap: true,
            ..Style::default()
        })
        .unwrap();
    let band =
        w.add_style(&Style { bold: true, align: Some(HAlign::CenterContinuous), fill: Some(Rgb(0xDDEBF7)), ..Style::default() }).unwrap();
    let money = w.add_style(&Style { num_fmt: Some("\"R$\" #,##0.00;[Red]-\"R$\" #,##0.00".into()), ..Style::default() }).unwrap();
    let pct = w.add_style(&Style { num_fmt: Some("0.0%".into()), align: Some(HAlign::End), ..Style::default() }).unwrap();
    let date = w.add_style(&Style { num_fmt: Some("dd/mm/yyyy".into()), ..Style::default() }).unwrap();
    let italic = w.add_style(&Style { italic: true, color: Some(Rgb(0x7F7F7F)), align: Some(HAlign::Start), ..Style::default() }).unwrap();
    let group = w
        .add_style(&Style {
            bold: true,
            border: Borders { bottom: Some(BorderEdge { style: BorderStyle::Medium, color: None }), ..Borders::default() },
            ..Style::default()
        })
        .unwrap();
    let total = w
        .add_style(&Style {
            bold: true,
            num_fmt: Some("#,##0.00".into()),
            border: Borders { top: Some(BorderEdge { style: BorderStyle::Medium, color: Some(Rgb(0x000000)) }), ..Borders::default() },
            ..Style::default()
        })
        .unwrap();
    let link = w.add_style(&Style { color: Some(Rgb(0x0563C1)), ..Style::default() }).unwrap();

    w.begin_sheet(SheetOptions {
        name: "Vendas & metas".into(),
        columns: vec![
            Column { width: Some(24.0), hidden: false },
            Column { width: Some(14.0), hidden: false },
            Column { width: Some(12.0), hidden: false },
            Column { width: Some(12.0), hidden: false },
            Column { width: Some(10.0), hidden: false },
            Column { width: Some(30.0), hidden: false },
        ],
        freeze_rows: 2,
        outline_levels: 1,
        print_title_rows: Some((1, 2)),
        names: vec![DefinedName { name: "ColumnTitle".into(), range: CellRange::cell(2, 1), sheet_scope: true }],
        ..SheetOptions::default()
    })
    .unwrap();
    let ro = RowOptions::default();
    // band row: label on the band's first column, centred across the empty styled cells
    w.append_row(
        &ro,
        &[
            Cell::text("Cliente").with_style(band),
            Cell::text("Valores").with_style(band),
            Cell::empty().with_style(band),
            Cell::empty().with_style(band),
            Cell::text("Outros").with_style(band),
            Cell::empty().with_style(band),
        ],
    )
    .unwrap();
    let heads = ["Nome", "Valor", "Meta", "Atingido", "Ativo", "Site"];
    let cells: Vec<Cell<'_>> = heads.iter().map(|h| Cell::text(h).with_style(header)).collect();
    w.append_row(&RowOptions { height: Some(30.0), ..ro }, &cells).unwrap();
    let data = [
        ("Ana Souza", 1234.5, 1000.0, true, "https://example.com/ana"),
        ("Bruno <&> Lima", -50.25, 200.0, false, "mailto:bruno@example.com"),
        ("  espaços  ", 0.1 + 0.2, 1e21, true, "javascript:alert(1)"),
        ("=SUM(A1:A2)", f64::NAN, 3.0, false, "'Vendas & metas'!A1"),
    ];
    w.append_row(&ro, &[Cell::text("Grupo: Sul (4)").with_style(group)]).unwrap();
    for (i, (name, v, meta, ativo, site)) in data.iter().enumerate() {
        let l = if site.contains('!') { Link::Location(site) } else { Link::Url(site) };
        w.append_row(
            &RowOptions { outline_level: 1, ..ro },
            &[
                Cell::text(name).with_style(if i == 0 { StyleId::DEFAULT } else { italic }),
                Cell::number(*v).with_style(money),
                Cell::number(*meta),
                Cell::number(v / meta).with_style(pct),
                Cell::new(CellValue::Bool(*ativo)),
                Cell::text(site).with_style(link).with_link(l),
            ],
        )
        .unwrap();
    }
    w.append_row(
        &RowOptions { outline_level: 1, ..ro },
        &[
            Cell::text("Datas"),
            Cell::new(CellValue::Date(DateTime::date(2026, 11, 23))).with_style(date),
            Cell::new(CellValue::Date(DateTime::date(2026, 11, 23))),
            Cell::new(CellValue::Date(DateTime { year: 2026, month: 11, day: 23, hour: 18, minute: 30, second: 0, millisecond: 0 })),
            Cell::new(CellValue::Date(DateTime::date(1899, 12, 31))),
        ],
    )
    .unwrap();
    w.append_row(&ro, &[Cell::text("Total").with_style(total), Cell::number(1184.25).with_style(total)]).unwrap();
    w.append_row(
        &RowOptions { hidden: true, ..ro },
        &[Cell::text("linha oculta"), Cell::text("tab\tand\nnewline"), Cell::text("ctl\u{1}_x0041_")],
    )
    .unwrap();
    w.merge(CellRange::new(20, 1, 21, 2)).unwrap();
    w.append_row(&ro, &[]).unwrap();
    w.end_sheet().unwrap();

    w.begin_sheet(SheetOptions {
        name: "עברית".into(),
        right_to_left: true,
        freeze_cols: 1,
        freeze_rows: 1,
        columns: vec![Column { width: Some(20.0), hidden: false }, Column { width: None, hidden: true }],
        ..SheetOptions::default()
    })
    .unwrap();
    w.append_row(
        &ro,
        &[Cell::text("שם").with_style(header), Cell::text("hidden col").with_style(header), Cell::text("ערך").with_style(header)],
    )
    .unwrap();
    w.append_row(&ro, &[Cell::text("start").with_style(italic), Cell::number(1.0), Cell::number(2.5).with_style(pct)]).unwrap();
    w.end_sheet().unwrap();
    w.finish().unwrap()
}

/// The clickCompliance pivot heat map: `rows` users × `cols` documents, a band row, a header
/// row, groups of 50 users with a summary row, every data cell styled from a 10-step scale.
pub fn heatmap(rows: u32, cols: u32, shared_strings: bool) -> Vec<u8> {
    let mut w = XlsxWriter::new(WorkbookOptions { shared_strings, ..workbook_options() }).unwrap();
    let header = w
        .add_style(&Style {
            bold: true,
            wrap: true,
            align: Some(HAlign::Center),
            border: Borders::all(BorderEdge { style: BorderStyle::Thin, color: None }),
            ..Style::default()
        })
        .unwrap();
    let band = w.add_style(&Style { bold: true, align: Some(HAlign::CenterContinuous), ..Style::default() }).unwrap();
    let group = w.add_style(&Style { bold: true, fill: Some(Rgb(0xEEEEEE)), ..Style::default() }).unwrap();
    let scale: Vec<StyleId> = (0..10u32)
        .map(|k| {
            let g = 255 - k * 20;
            w.add_style(&Style {
                fill: Some(Rgb((255 << 16) | (g << 8) | g)),
                num_fmt: Some("0%".into()),
                color: (k > 6).then_some(Rgb(0xFFFFFF)),
                align: Some(HAlign::Center),
                ..Style::default()
            })
            .unwrap()
        })
        .collect();
    let mut columns = vec![Column { width: Some(28.0), hidden: false }];
    columns.extend((0..cols).map(|_| Column { width: Some(9.0), hidden: false }));
    w.begin_sheet(SheetOptions {
        name: "Mapa de calor".into(),
        columns,
        freeze_rows: 2,
        freeze_cols: 1,
        outline_levels: 1,
        print_title_rows: Some((1, 2)),
        names: vec![DefinedName { name: "ColumnTitle".into(), range: CellRange::cell(2, 1), sheet_scope: true }],
        ..SheetOptions::default()
    })
    .unwrap();
    let ro = RowOptions::default();
    let mut band_cells = vec![Cell::empty()];
    for c in 0..cols {
        band_cells.push(
            if c % 10 == 0 {
                Cell::text(["Políticas", "Treinamentos", "Contratos", "Normas"][(c / 10 % 4) as usize])
            } else {
                Cell::empty()
            }
            .with_style(band),
        );
    }
    w.append_row(&ro, &band_cells).unwrap();
    let names: Vec<String> = (0..cols).map(|c| format!("Documento {:02}", c + 1)).collect();
    let mut head = vec![Cell::text("Usuário").with_style(header)];
    head.extend(names.iter().map(|n| Cell::text(n).with_style(header)));
    w.append_row(&ro, &head).unwrap();
    let mut seed = 0x1234_5678u32;
    for i in 0..rows {
        if i % 50 == 0 {
            let label = format!("Departamento: {} ({})", i / 50 + 1, 50.min(rows - i));
            w.append_row(&ro, &[Cell::text(&label).with_style(group)]).unwrap();
        }
        let user = format!("Usuário {:05} — usuario{i}@example.com", i + 1);
        let mut values = Vec::with_capacity(cols as usize);
        for _ in 0..cols {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            values.push(f64::from(seed % 101) / 100.0);
        }
        let mut cells = Vec::with_capacity(cols as usize + 1);
        cells.push(Cell::text(&user));
        cells.extend(values.iter().map(|&v| Cell::number(v).with_style(scale[((v * 9.99) as usize).min(9)])));
        w.append_row(&RowOptions { outline_level: 1, ..ro }, &cells).unwrap();
    }
    w.finish().unwrap()
}
