import React, {
  createContext,
  ReactNode,
  RefCallback,
  useCallback,
  useContext,
  useEffect,
  useRef,
} from "react";
import { useLocation } from "react-router-dom";

type URLFragment = string;

type RegisterScrollAnchorFn = (key: string, element: HTMLElement) => void;

const ScrollAnchorContext = createContext<RegisterScrollAnchorFn>(
  typeof WeakRef !== "undefined"
    ? (key: string) => {
        console.warn(
          `Ignoring registration of scroll anchor for key \`${key}\`.` +
            "Did you forget to wrap your app in a `ScrollAnchorProvider`?"
        );
      }
    : // This entire implementation gets disabled if WeakRef is not supported
      () => {}
);

export const ScrollAnchorProvider =
  typeof WeakRef !== "undefined"
    ? function ScrollAnchorProvider({ children }: { children: ReactNode }) {
        const location = useLocation();
        const registeredScrollTargets = useRef<{
          [fragment: URLFragment]: WeakRef<HTMLElement>[] | undefined;
        }>({});
        const scrollEnabled = useRef<boolean>(false);
        const maybeScroll = useCallback(() => {
          if (scrollEnabled.current === false) {
            return;
          }
          const targetsByFragment = registeredScrollTargets.current;
          const fragment = location.hash.replace(/^#/, "");
          const targets = targetsByFragment[fragment];
          if (!targets) {
            return;
          }
          targets.some((targetWeakRef) => {
            const target = targetWeakRef.deref();
            if (!target) {
              return false;
            }
            scrollEnabled.current = false;
            target.scrollIntoView();
            return true;
          });
        }, [location.hash]);
        const registerScrollAnchor = useCallback(
          (fragment: string, element: HTMLElement) => {
            const targetsByFragment = registeredScrollTargets.current;
            const targets = (targetsByFragment[fragment] =
              targetsByFragment[fragment] || []);
            targets.unshift(new WeakRef(element));
            maybeScroll();
          },
          [maybeScroll]
        );
        useEffect(() => {
          let distanceScrolled = 0;
          let lastKnownScrollPosition = window.scrollY;
          const handleScroll = () => {
            const currentScrollPosition = window.scrollY;
            distanceScrolled += Math.abs(
              lastKnownScrollPosition - currentScrollPosition
            );
            lastKnownScrollPosition = currentScrollPosition;
            if (distanceScrolled > 44) {
              // If the user has scrolled the page while waiting for the target
              // to appear during initial load, we do not want to steal control
              // away from them.
              scrollEnabled.current = false;
              window.removeEventListener("scroll", handleScroll);
            }
          };
          window.addEventListener("scroll", handleScroll, { passive: true });
          return () => {
            window.removeEventListener("scroll", handleScroll);
          };
        }, [location]);
        useEffect(() => {
          scrollEnabled.current = true;
          maybeScroll();
        }, [location, maybeScroll]);
        return (
          <ScrollAnchorContext.Provider value={registerScrollAnchor}>
            {children}
          </ScrollAnchorContext.Provider>
        );
      }
    : // This entire implementation gets disabled if WeakRef is not supported
      React.Fragment;

export function useScrollAnchor(key: URLFragment): RefCallback<HTMLElement> {
  const registerScrollTarget = useContext(ScrollAnchorContext);
  return useCallback(
    (instance) => {
      if (!instance) {
        return;
      }
      registerScrollTarget(key, instance);
    },
    [key, registerScrollTarget]
  );
}
