/**
 * 字幕バーのエントリ。
 *
 * スタジオとは別の HTML にしてある。`?window=caption` のような
 * クエリでの振り分けは vite の dev サーバでは通るが、配布ビルドの
 * アセット解決では失敗する (パスとして扱われる)。
 */
import { ApolloProvider } from "@apollo/client/react";
import React from "react";
import ReactDOM from "react-dom/client";

import { CaptionWindow } from "./CaptionWindow";
import { client } from "./lib/apollo";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ApolloProvider client={client}>
      <CaptionWindow />
    </ApolloProvider>
  </React.StrictMode>,
);
