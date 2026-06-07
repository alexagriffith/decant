// Transcript navigation: highlights the active turn in the table of contents
// as you scroll (one IntersectionObserver over the turn anchors), and wires the
// Expand all / Collapse all buttons. All client-side over the static DOM.
export default {
  mounted() {
    this.onClick = (e) => {
      if (e.target.closest("[data-expand-all]")) this.toggleAll(true)
      else if (e.target.closest("[data-collapse-all]")) this.toggleAll(false)
    }
    this.el.addEventListener("click", this.onClick)
    this.observe()
  },

  updated() {
    this.observe()
  },

  destroyed() {
    if (this.io) this.io.disconnect()
    this.el.removeEventListener("click", this.onClick)
  },

  toggleAll(open) {
    this.el.querySelectorAll("article details").forEach((d) => {
      d.open = open
    })
  },

  observe() {
    if (this.io) this.io.disconnect()
    this.links = Array.from(this.el.querySelectorAll("[data-toc]"))
    const targets = this.links
      .map((l) => this.el.querySelector(l.getAttribute("href")))
      .filter(Boolean)
    if (targets.length === 0) return

    this.io = new IntersectionObserver(
      (entries) => {
        const hit = entries.find((e) => e.isIntersecting)
        if (hit) this.setActive(hit.target.id)
      },
      { rootMargin: "-180px 0px -65% 0px", threshold: 0 }
    )
    targets.forEach((t) => this.io.observe(t))
  },

  setActive(id) {
    this.links.forEach((l) => {
      l.setAttribute("data-active", l.getAttribute("href") === "#" + id ? "true" : "false")
    })
  },
}
