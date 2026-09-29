import { ApolloProvider } from "@apollo/client/react";
import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./App";
import { CaptionWindow } from "./CaptionWindow";
import { client } from "./lib/apollo";
import "./index.css";

// Tauri は字幕バーを `?window=caption` で開く。
// vite 単体で動かすときは URL に付けて切り替えられる。
const isCaption =
  new URLSearchParams(window.location.search).get("window") === "caption";

if (isCaption) {
  document.body.classList.add("caption-window");
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ApolloProvider client={client}>
      {isCaption ? <CaptionWindow /> : <App />}
    </ApolloProvider>
  </React.StrictMode>,
);
