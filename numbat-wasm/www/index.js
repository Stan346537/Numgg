import { setup_panic_hook, Numbat } from "numbat-wasm";

setup_panic_hook();

var numbat = Numbat.new();

// Load KeyboardEvent polyfill for old browsers
keyboardeventKeyPolyfill.polyfill();
  
function updateUrlQuery(query) {
  let url = new URL(window.location);
  if (query == null) {
    url.searchParams.delete('q');
  } else {
    url.searchParams.set('q', query);
  }

  history.replaceState(null, null, url);
}

function interpret(line) {
  // Skip empty lines or line comments
  var lineTrimmed = line.trim();
  if (lineTrimmed === "" || lineTrimmed[0] === "#") {
    return;
  }

  if (lineTrimmed == "clear") {
    this.clear();
    var output = "";
  } else {
    var output = numbat.interpret(line);
  }

  updateUrlQuery(line);

  return output;
}

$(document).ready(function() {
  var term = $('#terminal').terminal(interpret, {
    greetings: false,
    name: "terminal",
    height: 550,
    prompt: "[[;;;prompt]>>> ]",
    checkArity: false,
    historySize: 200,
    historyFilter(line) {
      return line.trim() !== "";
    },
    completion(inp, cb) {
      cb(numbat.get_completions_for(inp));
    },
    onClear() {
      updateUrlQuery(null);
    }
  });

  // evaluate expression in query string if supplied (via opensearch)
  if (location.search) {
    var queryParams = new URLSearchParams(location.search);
    if (queryParams.has("q")) {
      term.exec(queryParams.get("q"));
    }
  }
});
