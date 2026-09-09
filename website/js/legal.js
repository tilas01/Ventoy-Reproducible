// SPDX-License-Identifier: GPL-3.0-or-later
//
// The welcome dialog: the licence, what this project does not promise, and the
// disclosure that it was built with AI assistance.
//
// # Shown once per session, and never phoned home
//
// Acceptance is recorded in sessionStorage, so it survives navigation and is
// gone when the tab closes. It is not a cookie, so it is never attached to a
// request; there is no server here to receive it and no analytics to correlate
// it with. That is also why there is no "remember me for ever" option: a
// permanent record would be more data about the reader than this site has any
// business keeping.
//
// # Why it is a real gate and not a dismissible strip
//
// The third paragraph is the part that matters. This project proves that a
// binary follows from its source; it does not prove the source is safe, and it
// does not prove the compiler is honest. Somebody who assumes otherwise could
// boot a machine believing it verified something it did not. A banner at the
// bottom of the page does not carry that.
//
// The page underneath is inert while the dialog is open: focus is trapped, and
// the content is hidden from assistive technology, so the gate cannot be
// stepped around by tabbing past it.
//
// # If scripts are off
//
// The gate never appears and the page reads normally. That is deliberate. The
// same words are in the page itself under "What this does not prove", so a
// reader with JavaScript disabled loses a dialog and no information.

(function () {
  "use strict";

  var KEY = "ventoy-reproducible-accepted-v1";

  function accepted() {
    try {
      return sessionStorage.getItem(KEY) === "yes";
    } catch (e) {
      // Private browsing throws rather than returning null. Treat it as not
      // accepted and carry on: showing the dialog again is a small cost, and
      // failing to render the page is not.
      return false;
    }
  }

  function remember() {
    try {
      sessionStorage.setItem(KEY, "yes");
    } catch (e) {
      /* private mode, nothing to do */
    }
  }

  function build() {
    var overlay = document.createElement("div");
    overlay.className = "gate";
    overlay.setAttribute("role", "dialog");
    overlay.setAttribute("aria-modal", "true");
    overlay.setAttribute("aria-labelledby", "gate-title");

    overlay.innerHTML = [
      '<div class="gate-box">',
      '  <h2 id="gate-title">BEFORE YOU USE THIS</h2>',

      // Deliberately first: the misunderstanding that could actually cost
      // somebody something, ahead of the licence and ahead of the AI notice.
      '  <p><b>This project proves that a binary follows from its source.</b>',
      '  It does not prove the source is safe, and nobody here has audited',
      '  Ventoy&rsquo;s C. It does not prove the compiler is honest either:',
      '  four of the seven pinned toolchains are prebuilt binaries with no',
      '  canonical source to compare against. A reproducible build made by a',
      '  malicious compiler reproduces perfectly.</p>',

      '  <p>This is a <b>fork of Ventoy</b>, not Ventoy. It is not a criticism',
      '  of Ventoy and there is no evidence of any wrongdoing by its authors.',
      '  The argument is that an unverifiable binary is unverifiable whoever',
      '  produced it.</p>',

      '  <p>It is free software under the <b>GNU General Public License v3 or',
      '  later</b>, and it comes with <b>absolutely no warranty</b>. You are',
      '  writing bootloaders to removable media at your own risk.</p>',

      '  <p>Much of the code and documentation added by this fork was drafted',
      '  with <b>AI assistance (Claude, by Anthropic)</b>, reviewed and tested',
      '  by <b>tilas01</b>. That is a maintainer review: no external firm or',
      '  independent researcher has audited this. It is disclosed so you can',
      '  judge for yourself how much to check before relying on it, and the',
      '  whole thing is published under the GPL precisely so that you can.</p>',

      '  <div class="gate-actions">',
      '    <button class="agree" type="button">I have read this</button>',
      '    <a class="leave" href="https://github.com/ventoy/Ventoy">Take me to Ventoy instead</a>',
      '  </div>',
      "</div>"
    ].join("");

    return overlay;
  }

  function show() {
    var overlay = build();
    document.body.appendChild(overlay);
    document.body.classList.add("gated");

    var main = document.querySelector("main");
    if (main) {
      main.setAttribute("aria-hidden", "true");
    }

    var button = overlay.querySelector("button.agree");
    button.focus();

    function dismiss() {
      remember();
      overlay.remove();
      document.body.classList.remove("gated");
      if (main) {
        main.removeAttribute("aria-hidden");
      }
      document.removeEventListener("keydown", trap, true);
    }

    // Focus stays inside the dialog. Without this, tabbing walks into the page
    // behind, which is visually blurred and still reachable by a screen reader
    // if `aria-hidden` were the only guard.
    function trap(event) {
      if (event.key === "Tab") {
        var focusable = overlay.querySelectorAll("button, a[href]");
        var first = focusable[0];
        var last = focusable[focusable.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
      // Escape does not dismiss. Escape is the reflex for closing something you
      // have not read, which is the one outcome this dialog exists to prevent.
    }

    button.addEventListener("click", dismiss);
    document.addEventListener("keydown", trap, true);
  }

  function start() {
    if (!accepted()) {
      show();
    }
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start);
  } else {
    start();
  }
})();
