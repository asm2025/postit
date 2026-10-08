// Applies the saved theme before the bundle loads, so the first paint is already right.
;(function () {
  var theme = 'system'
  try {
    var saved = localStorage.getItem('postit.theme')
    if (saved === 'light' || saved === 'dark') theme = saved
  } catch (e) {
    /* storage unavailable */
  }
  var dark = theme === 'dark' || (theme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches)
  document.documentElement.classList.toggle('dark', dark)
})()
