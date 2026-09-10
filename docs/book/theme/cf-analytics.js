// Cloudflare Web Analytics (cookieless) for the docs under /docs.
// Same token as site/index.html; injected here because mdBook has no head hook
// short of overriding index.hbs.
(function () {
  var s = document.createElement("script");
  s.defer = true;
  s.src = "https://static.cloudflareinsights.com/beacon.min.js";
  s.setAttribute("data-cf-beacon", '{"token": "CLOUDFLARE_WEB_ANALYTICS_TOKEN"}');
  document.head.appendChild(s);
})();
