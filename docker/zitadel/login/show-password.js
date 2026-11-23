// Adds a show/hide eye to every password field on Zitadel's built-in login pages.
// Injected by postit-nginx-infra (infra.conf); served from the same origin so the login's CSP allows it.
(function () {
    var NS = "http://www.w3.org/2000/svg";
    var EYE = ["M1 12s4-7 11-7 11 7 11 7-4 7-11 7S1 12 1 12z", "M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z"];
    var EYE_OFF = [
        "M17.94 17.94A10.9 10.9 0 0 1 12 19C5 19 1 12 1 12a19.8 19.8 0 0 1 5.06-5.94",
        "M9.9 4.24A10.7 10.7 0 0 1 12 5c7 0 11 7 11 7a19.7 19.7 0 0 1-3.17 4.19",
        "M14.12 14.12A3 3 0 1 1 9.88 9.88",
        "M1 1l22 22"
    ];

    function icon(paths) {
        var svg = document.createElementNS(NS, "svg");
        svg.setAttribute("viewBox", "0 0 24 24");
        svg.setAttribute("aria-hidden", "true");
        paths.forEach(function (d) {
            var p = document.createElementNS(NS, "path");
            p.setAttribute("d", d);
            svg.appendChild(p);
        });
        return svg;
    }

    function attach(input) {
        if (input.dataset.postitPw) return;
        input.dataset.postitPw = "1";

        var wrap = document.createElement("span");
        wrap.className = "postit-pw-wrap";
        input.parentNode.insertBefore(wrap, input);
        wrap.appendChild(input);

        var btn = document.createElement("button");
        btn.type = "button";
        btn.className = "postit-pw-toggle";
        btn.setAttribute("aria-label", "Show password");
        btn.setAttribute("aria-pressed", "false");
        btn.appendChild(icon(EYE));
        wrap.appendChild(btn);

        function setShown(show) {
            input.type = show ? "text" : "password";
            btn.setAttribute("aria-label", show ? "Hide password" : "Show password");
            btn.setAttribute("aria-pressed", show ? "true" : "false");
            btn.replaceChild(icon(show ? EYE_OFF : EYE), btn.firstChild);
        }

        btn.addEventListener("click", function () {
            setShown(input.type === "password");
            input.focus();
        });

        // Submit as a password field again, so password managers recognize and offer to save it.
        if (input.form) {
            input.form.addEventListener("submit", function () {
                if (input.type !== "password") setShown(false);
            });
        }
    }

    function init() {
        document.querySelectorAll('input[type="password"]').forEach(attach);
    }

    if (document.readyState === "loading") {
        document.addEventListener("DOMContentLoaded", init);
    } else {
        init();
    }
})();
