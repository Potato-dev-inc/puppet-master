/* Tiny, dependency-free. Two jobs: the theme toggle and the (not yet connected) waitlist form. */
(function () {
  'use strict';
  var root = document.documentElement;

  function currentTheme() {
    var set = root.getAttribute('data-theme');
    if (set === 'light' || set === 'dark') return set;
    return window.matchMedia && window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  }

  var toggle = document.getElementById('theme-toggle');
  if (toggle) {
    toggle.addEventListener('click', function () {
      var next = currentTheme() === 'dark' ? 'light' : 'dark';
      root.setAttribute('data-theme', next);
      try { localStorage.setItem('pm-site-theme', next); } catch (e) { /* private mode: the toggle still works for this visit */ }
    });
  }

  /* Waitlist. The form's action is "#" until a real endpoint exists. Until then nothing is sent anywhere:
     the page says so plainly. To connect it, set the form's action (and method) in the template; this
     handler steps aside as soon as the action is not "#". */
  var form = document.getElementById('waitlist-form');
  var msg = document.getElementById('waitlist-msg');
  if (form && msg) {
    form.addEventListener('submit', function (ev) {
      var email = form.elements.email;
      if (!email.value || !/^[^@\s]+@[^@\s]+\.[^@\s]+$/.test(email.value)) {
        ev.preventDefault();
        msg.textContent = 'Please enter a valid email address.';
        email.focus();
        return;
      }
      if ((form.getAttribute('action') || '#') === '#') {
        ev.preventDefault();
        msg.textContent = 'This is a draft page, so the waitlist is not connected yet. Nothing was sent.';
      }
    });
  }
})();
