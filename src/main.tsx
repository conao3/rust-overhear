/** スタジオ (履歴・語彙・設定) のエントリ。字幕バーは caption.tsx。 */
import { ApolloProvider } from "@apollo/client/react";
import React from "react";
import ReactDOM from "react-dom/client";

import { App } from "./App";
import { client } from "./lib/apollo";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ApolloProvider client={client}>
      <App />
    </ApolloProvider>
  </React.StrictMode>,
);
