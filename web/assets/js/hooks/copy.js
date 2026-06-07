// Copy-to-clipboard hook. Put phx-hook="Copy" + data-copy="<text>" on a button;
// an optional child with [data-copy-label] gets a brief "Copied!" swap.
export default {
  mounted() {
    this.el.addEventListener("click", () => {
      const text = this.el.dataset.copy || ""
      navigator.clipboard.writeText(text).then(() => {
        const label = this.el.querySelector("[data-copy-label]")
        if (label) {
          const orig = label.textContent
          label.textContent = "Copied!"
          setTimeout(() => {
            label.textContent = orig
          }, 1200)
        }
      })
    })
  },
}
