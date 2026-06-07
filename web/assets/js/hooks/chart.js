// Theme-aware ECharts hook. The server renders <.chart id spec={…}> with a
// normalized spec in data-spec; this hook builds the ECharts option, pulls
// colors from the design-system CSS variables, and re-themes on theme change.
import * as echarts from "echarts"

function cssVars() {
  const s = getComputedStyle(document.documentElement)
  const v = (n) => s.getPropertyValue(n).trim()
  return {
    fg: v("--color-fg"),
    muted: v("--color-muted"),
    faint: v("--color-faint"),
    line: v("--color-line"),
    surface: v("--color-surface"),
    accent: v("--color-accent"),
    info: v("--color-info"),
    success: v("--color-success"),
    warning: v("--color-warning"),
  }
}

function compact(n) {
  n = +n
  const a = Math.abs(n)
  if (a >= 1e6) return (n / 1e6).toFixed(1) + "M"
  if (a >= 1e3) return (n / 1e3).toFixed(1) + "k"
  return String(n)
}

function buildOption(spec, c) {
  const money = spec.y_format === "money"
  const fmtY = money ? (v) => "$" + compact(v) : (v) => compact(v)

  return {
    color: [c.accent, c.info, c.success, c.warning],
    textStyle: { fontFamily: "inherit", color: c.muted },
    grid: { left: 6, right: 16, top: 18, bottom: 6, containLabel: true },
    tooltip: {
      trigger: "axis",
      backgroundColor: c.surface,
      borderColor: c.line,
      borderWidth: 1,
      padding: [8, 12],
      textStyle: { color: c.fg, fontSize: 12 },
      axisPointer: { type: "shadow", shadowStyle: { color: c.fg + "0d" } },
      valueFormatter: money ? (v) => "$" + Number(v).toFixed(2) : (v) => String(v),
    },
    xAxis: {
      type: "category",
      data: spec.categories || [],
      boundaryGap: spec.type !== "line",
      axisLine: { lineStyle: { color: c.line } },
      axisTick: { show: false },
      axisLabel: { color: c.faint, hideOverlap: true, fontSize: 11 },
    },
    yAxis: {
      type: "value",
      axisLabel: { color: c.faint, formatter: fmtY, fontSize: 11 },
      splitLine: { lineStyle: { color: c.line, type: "dashed" } },
    },
    series: (spec.series || []).map((s) => {
      const type = s.type || spec.type || "bar"
      return {
        name: s.name,
        type,
        data: s.data,
        smooth: spec.smooth ?? true,
        showSymbol: false,
        barMaxWidth: 26,
        itemStyle: { borderRadius: type === "bar" ? [3, 3, 0, 0] : 0 },
        lineStyle: { width: 2 },
        areaStyle: type === "line" ? { opacity: 0.1 } : undefined,
      }
    }),
  }
}

export default {
  mounted() {
    this.onTheme = () => this.draw()
    this.onResize = () => this.chart && this.chart.resize()
    window.addEventListener("phx:set-theme", this.onTheme)
    window.addEventListener("resize", this.onResize)
    this.mql = window.matchMedia("(prefers-color-scheme: dark)")
    this.mql.addEventListener("change", this.onTheme)
    // Defer one frame so CSS variables are resolved.
    requestAnimationFrame(() => this.draw())
  },
  updated() {
    this.draw()
  },
  destroyed() {
    if (this.chart) {
      this.chart.dispose()
      this.chart = null
    }
    window.removeEventListener("phx:set-theme", this.onTheme)
    window.removeEventListener("resize", this.onResize)
    this.mql && this.mql.removeEventListener("change", this.onTheme)
  },
  draw() {
    let spec
    try {
      spec = JSON.parse(this.el.dataset.spec || "{}")
    } catch (_e) {
      spec = {}
    }
    if (!this.chart) this.chart = echarts.init(this.el, null, { renderer: "canvas" })
    this.chart.setOption(buildOption(spec, cssVars()), true)
    this.chart.resize()
  },
}
