/**
 * Apollo Client の配線。
 *
 * subscription は graphql-ws で WebSocket、それ以外は HTTP。
 * Segment を id で正規化しておくことで、ASR の interim → final の
 * 差し替えが「同じ id の更新」としてキャッシュに入る。
 */
import { ApolloClient, InMemoryCache, split } from "@apollo/client";
import { HttpLink } from "@apollo/client/link/http";
import { GraphQLWsLink } from "@apollo/client/link/subscriptions";
import { getMainDefinition } from "@apollo/client/utilities";
import { createClient } from "graphql-ws";

import { endpoint } from "./config";

const httpLink = new HttpLink({
  uri: endpoint.graphql,
  headers: endpoint.token ? { Authorization: `Bearer ${endpoint.token}` } : {},
});

const wsLink = new GraphQLWsLink(
  createClient({
    url: endpoint.websocket,
    // トークンは connectionParams で渡す (ヘッダを付けられないため)。
    connectionParams: () => (endpoint.token ? { token: endpoint.token } : {}),
    retryAttempts: Infinity,
  }),
);

const link = split(
  ({ query }) => {
    const definition = getMainDefinition(query);
    return (
      definition.kind === "OperationDefinition" &&
      definition.operation === "subscription"
    );
  },
  wsLink,
  httpLink,
);

export const client = new ApolloClient({
  link,
  cache: new InMemoryCache({
    typePolicies: {
      Segment: { keyFields: ["id"] },
    },
  }),
});
