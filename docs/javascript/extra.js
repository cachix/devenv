document$.subscribe(() => {
  highlightCodeOnHomePage();
});

target$.subscribe(() => {
  highlightCodeOnHomePage();
});

function highlightCodeOnHomePage() {
  if (document.location.pathname === '/') {
    hljs.highlightAll();
  }
}

