// Applies the saved theme before the bundle loads, so the first paint is already right.
;(function () {
  var theme = 'system'
  try {
    theme = localStorage.getItem('postit.theme') || 'system'
  } catch (e) {
    /* storage unavailable */
  }
  var dark = theme === 'dark' || (theme !== 'light' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.classList.toggle('dark', dark)
})()
