//! `summarize(space)` of `slides/packages/slides-core/src/chart-engine.ts:79`: the summary both hosts
//! read (type, title, series with their solid colours, categories, legend position, the first value and
//! category axes) so a caller that only needs "what chart is this" never walks the model.

use uo_jsnum::format::to_js_string;

use crate::model::{Axis, AxisKind, BarDir, ChartColor, ChartSpace, GroupKind, LegendPos};

/// `ChartType` of chart-engine.ts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChartType {
    BarCol,
    BarRow,
    Line,
    Pie,
    Doughnut,
    Scatter,
    Radar,
    Area,
    Unknown,
}

/// A series of the summary. `fill`/`line`: absent (`None`), `null` = no fill/line (`Some(None)`) or a
/// solid colour (`Some(Some(c))`).
#[derive(Clone, PartialEq, Debug)]
pub struct SeriesSummary {
    pub name: Box<str>,
    /// the values, blanks as 0
    pub values: Vec<f64>,
    pub color: Option<ChartColor>,
    pub fill: Option<Option<ChartColor>>,
    pub line: Option<Option<ChartColor>>,
}

/// `ChartTextStyle`.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct AxisText {
    pub size_pt: Option<f64>,
    pub color: Option<ChartColor>,
    pub font: Option<Box<str>>,
}

/// `ChartAxis`.
#[derive(Clone, PartialEq, Debug)]
pub struct AxisSummary {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub major_unit: Option<f64>,
    pub log_base: Option<f64>,
    pub grid: bool,
    pub deleted: bool,
    pub text: Option<AxisText>,
}

/// `ChartModel` minus the space.
#[derive(Clone, PartialEq, Debug)]
pub struct ChartSummary {
    pub ty: ChartType,
    pub title: Option<String>,
    pub series: Vec<SeriesSummary>,
    pub categories: Vec<String>,
    pub legend_pos: Option<LegendPos>,
    pub val_ax: Option<AxisSummary>,
    pub cat_ax: Option<AxisSummary>,
}

/// `summarize(space)`.
pub fn summarize(space: &ChartSpace) -> ChartSummary {
    let ty = match space.groups.first() {
        None => ChartType::Unknown,
        Some(g0) => match g0.kind {
            GroupKind::Bar => {
                if g0.dir == BarDir::Bar {
                    ChartType::BarRow
                } else {
                    ChartType::BarCol
                }
            }
            GroupKind::Line | GroupKind::Stock => ChartType::Line,
            GroupKind::OfPie | GroupKind::Pie => ChartType::Pie,
            GroupKind::Doughnut => ChartType::Doughnut,
            GroupKind::Scatter => ChartType::Scatter,
            GroupKind::Radar => ChartType::Radar,
            GroupKind::Area => ChartType::Area,
            _ => ChartType::Unknown,
        },
    };
    let mut series = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    for g in &space.groups {
        for s in &g.series {
            let mut cs = SeriesSummary {
                name: s.name.clone(),
                values: s.values.iter().map(|v| v.unwrap_or(0.0)).collect(),
                color: None,
                fill: None,
                line: None,
            };
            match &s.sp.fill {
                Some(crate::model::Paint::None) => cs.fill = Some(None),
                Some(crate::model::Paint::Solid { color, .. }) => {
                    cs.fill = Some(Some(color.clone()));
                    cs.color = Some(color.clone());
                }
                _ => {}
            }
            match s.sp.line.as_ref().and_then(|l| l.paint.as_ref()) {
                Some(crate::model::Paint::None) => cs.line = Some(None),
                Some(crate::model::Paint::Solid { color, .. }) => {
                    cs.line = Some(Some(color.clone()));
                    if cs.color.is_none() {
                        cs.color = Some(color.clone());
                    }
                }
                _ => {}
            }
            series.push(cs);
            if categories.is_empty() {
                categories = match (&s.cats, &s.cat_nums, &s.x_vals) {
                    (Some(c), _, _) => c.iter().map(|c| c.to_string()).collect(),
                    (None, Some(n), _) | (None, None, Some(n)) => n.iter().map(|v| v.map_or_else(String::new, to_js_string)).collect(),
                    (None, None, None) => Vec::new(),
                };
            }
        }
    }
    // `paras.map(runs.join('')).join('\n') || text`
    let title = space.title.as_ref().and_then(|t| {
        let rich = t
            .paras
            .as_ref()
            .map(|ps| ps.iter().map(|p| p.runs.iter().map(|r| &*r.text).collect::<String>()).collect::<Vec<_>>().join("\n"));
        match rich {
            Some(r) if !r.is_empty() => Some(r),
            _ => t.text.as_ref().filter(|s| !s.is_empty()).map(|s| s.to_string()),
        }
    });
    let ax = |val: bool| -> Option<AxisSummary> {
        let a: &Axis = space.axes.iter().find(|x| ((x.kind == AxisKind::Val) == val) && !x.id.starts_with("__"))?;
        let t = &a.tx_pr;
        let text = (t.sz.is_some() || t.color.is_some() || t.font.is_some()).then(|| AxisText {
            size_pt: t.sz,
            color: t.color.clone(),
            font: t.font.clone(),
        });
        Some(AxisSummary {
            min: a.min,
            max: a.max,
            major_unit: a.major_unit,
            log_base: a.log_base,
            grid: a.major_grid.is_some(),
            deleted: a.deleted,
            text,
        })
    };
    ChartSummary {
        ty,
        title,
        series,
        categories,
        legend_pos: space.legend.as_ref().map(|l| l.pos.clone()),
        val_ax: ax(true),
        cat_ax: ax(false),
    }
}
