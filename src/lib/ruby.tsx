import { useMutation, useSubscription } from "@apollo/client/react";
import {
  createContext,
  useContext,
  type CSSProperties,
  type ReactNode,
} from "react";

import { RUBY_SETTINGS_UPDATES, SET_RUBY_SETTINGS } from "./queries";
import type { RubySettings } from "./types";

const RubyContext = createContext<{
  settings: RubySettings | undefined;
  save: (settings: RubySettings) => Promise<void>;
  saving: boolean;
  error: string | undefined;
} | null>(null);

export function RubyProvider({ children }: { children: ReactNode }) {
  const { data, error: subscriptionError } = useSubscription<{
    rubySettingsUpdates: RubySettings;
  }>(RUBY_SETTINGS_UPDATES);
  const [mutate, { loading: saving, error: mutationError }] =
    useMutation(SET_RUBY_SETTINGS);
  const settings = data?.rubySettingsUpdates;
  const save = async (next: RubySettings) => {
    await mutate({ variables: next });
  };
  return (
    <RubyContext.Provider
      value={{
        settings,
        save,
        saving,
        error: mutationError?.message ?? subscriptionError?.message,
      }}
    >
      <div
        className="contents"
        style={
          {
            "--ruby-size": `${(settings?.size ?? 75) / 100}em`,
          } as CSSProperties
        }
      >
        {children}
      </div>
    </RubyContext.Provider>
  );
}

export function useRuby() {
  const context = useContext(RubyContext);
  if (!context) throw new Error("RubyProvider is missing");
  return context;
}
