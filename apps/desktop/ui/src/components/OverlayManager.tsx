import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import ReactDOM from "react-dom";

type OverlayResolve<T = any> = (value: T | null) => void;

type OverlayEntry = {
  id: number;
  Component: React.ComponentType<any>;
  props?: any;
  resolve: OverlayResolve;
  restoreFocusTo: HTMLElement | null;
};

type OverlayContextValue = {
  dismissTopOverlay: (value?: any) => boolean;
  hasOpenOverlay: boolean;
  openOverlay: <T = any>(
    Component: React.ComponentType<any>,
    props?: any,
  ) => Promise<T | null>;
};

const OverlayContext = createContext<OverlayContextValue>({
  dismissTopOverlay: () => false,
  hasOpenOverlay: false,
  openOverlay: () => Promise.resolve(null),
});

export const useOverlay = () => useContext(OverlayContext);

export const OverlayProvider: React.FC<
  React.PropsWithChildren<Record<string, unknown>>
> = ({ children }) => {
  const [stack, setStack] = useState<OverlayEntry[]>([]);
  const idRef = useRef(1);
  const stackRef = useRef<OverlayEntry[]>([]);

  useEffect(() => {
    if (typeof document === "undefined") return;
    const body = document.body;
    if (stack.length > 0) {
      const prev = body.style.overflow;
      body.style.overflow = "hidden";
      return () => {
        body.style.overflow = prev;
      };
    }
  }, [stack.length]);

  const openOverlay = useCallback(
    <T,>(
      Component: React.ComponentType<any>,
      props?: any,
    ): Promise<T | null> => {
      const id = idRef.current++;
      const restoreFocusTo =
        typeof document !== "undefined" &&
        document.activeElement instanceof HTMLElement
          ? document.activeElement
          : null;

      return new Promise<T | null>((resolve) => {
        const entry: OverlayEntry = {
          id,
          Component,
          props,
          resolve,
          restoreFocusTo,
        };
        setStack((currentStack) => {
          const nextStack = [...currentStack, entry];
          stackRef.current = nextStack;
          return nextStack;
        });
      });
    },
    [],
  );

  const closeById = useCallback((id: number, value: any) => {
    const closedEntry = stackRef.current.find((entry) => entry.id === id);

    setStack((currentStack) => {
      const nextStack = currentStack.filter((entry) => entry.id !== id);
      stackRef.current = nextStack;
      return nextStack;
    });

    if (!closedEntry) {
      return;
    }

    closedEntry.resolve(value);

    if (
      closedEntry.restoreFocusTo &&
      document.contains(closedEntry.restoreFocusTo)
    ) {
      requestAnimationFrame(() => {
        closedEntry?.restoreFocusTo?.focus();
      });
    }
  }, []);

  const dismissTopOverlay = useCallback(
    (value: any = null) => {
      const topEntry = stackRef.current[stackRef.current.length - 1];

      if (!topEntry) {
        return false;
      }

      closeById(topEntry.id, value);
      return true;
    },
    [closeById],
  );

  const overlays = (
    <>
      {stack.map((entry) => {
        const { id, Component, props } = entry;
        const overlayNode = (
          <div
            aria-hidden={false}
            className="overlay-manager__entry"
            key={id}
            style={{ zIndex: 6000 + id }}
          >
            <div className="overlay-manager__backdrop" />
            <div className="overlay-manager__content">
              <Component
                {...props}
                onResolve={(overlayValue: any) => closeById(id, overlayValue)}
              />
            </div>
          </div>
        );
        if (typeof document !== "undefined") {
          return ReactDOM.createPortal(
            overlayNode,
            document.body as HTMLElement,
          );
        }
        return overlayNode;
      })}
    </>
  );

  return (
    <OverlayContext.Provider
      value={{
        dismissTopOverlay,
        hasOpenOverlay: stack.length > 0,
        openOverlay,
      }}
    >
      {children}
      {overlays}
    </OverlayContext.Provider>
  );
};

export default OverlayProvider;
