// Cloudflare Web Analytics (cookieless) for the docs under /docs.
// Same token as site/index.html; injected here because mdBook has no head hook
// short of overriding index.hbs.
(function () {
  var s = document.createElement("script");
  s.type = "module";
  s.src = "https://static.cloudflareinsights.com/beacon.min.js";
  s.setAttribute("data-cf-beacon", '{"token": "f4af87acd28e490593656ca9636659f8"}');
  document.head.appendChild(s);
})();
