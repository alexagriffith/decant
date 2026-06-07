// Copy-to-clipboard hook. Put phx-hook="Copy" + data-copy="<text>" on a button.
// A child marked [data-copy-icon] briefly swaps from the clipboard glyph to a
// check, and a child marked [data-copy-label] gets a brief "Copied" text swap.
// Either, both, or neither may be present.
export default {
  mounted() {
    this.el.addEventListener("click", () => {
      const text = this.el.dataset.copy || ""
      navigator.clipboard.writeText(text).then(() => this.flash())
    })
  },

  flash() {
    const icon = this.el.querySelector("[data-copy-icon]")
    if (icon) {
      icon.classList.remove("hero-clipboard-document")
      icon.classList.add("hero-check", "text-success")
      setTimeout(() => {
        icon.classList.remove("hero-check", "text-success")
        icon.classList.add("hero-clipboard-document")
      }, 1200)
    }

    const label = this.el.querySelector("[data-copy-label]")
    if (label) {
      const orig = label.textContent
      label.textContent = "Copied"
      setTimeout(() => {
        label.textContent = orig
      }, 1200)
    }
  },
}
