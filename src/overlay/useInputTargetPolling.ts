import { useEffect, useRef, useState } from "react";
import {
  getFrontmostApp,
  getCurrentInputTarget,
  showPromptButton,
  hidePromptButton,
  hidePromptPopover,
} from "../platform/platformApi";
import type { FrontmostApp } from "../platform/platformApi";

interface InputTarget {
  frame: { x: number; y: number; width: number; height: number };
  window_frame: { x: number; y: number; width: number; height: number };
  button_position: [number, number];
  app: FrontmostApp | null;
}

type OverlayPlacement = {
  buttonOffset: { x: number; y: number } | null;
};

interface InputTargetPollingOptions {
  onBasePositionChange?: (position: [number, number]) => void;
}

const DEFAULT_BUTTON_POSITION: [number, number] = [960, 700];

export function useInputTargetPolling(
  blacklist: string[] = [],
  overlayPlacement: OverlayPlacement = { buttonOffset: null },
  options: InputTargetPollingOptions = {},
  floatingButtonVisible = true
) {
  const [target, setTarget] = useState<InputTarget | null>(null);
  const [showAttached, setShowAttached] = useState(false);
  const lastTargetAppRef = useRef<FrontmostApp | null>(null);
  const lastButtonPositionRef = useRef<[number, number] | null>(DEFAULT_BUTTON_POSITION);
  const lastTargetAtRef = useRef(0);
  const pollingRef = useRef<boolean>(true);
  const activeRef = useRef(true);
  const prevVisibilityRef = useRef(floatingButtonVisible);

  useEffect(() => {
    activeRef.current = true;
    pollingRef.current = true;

    const poll = async () => {
      if (!activeRef.current) return;

      if (!floatingButtonVisible) {
        setTarget(null);
        setShowAttached(false);
        await hidePromptButton();
        await hidePromptPopover();
        pollingRef.current = false;
        return;
      }

      if (!pollingRef.current) {
        pollingRef.current = true;
      }

      try {
        const app = await getFrontmostApp();

        const inputTarget = (await getCurrentInputTarget()) as InputTarget | null;

        if (inputTarget && app) {
          setTarget(inputTarget);
          setShowAttached(true);
          lastTargetAppRef.current = inputTarget.app;
          const [x, y] = inputTarget.button_position;
          const offset = overlayPlacement.buttonOffset;
          const displayX = offset ? x + offset.x : x;
          const displayY = offset ? y + offset.y : y;
          lastButtonPositionRef.current = [displayX, displayY];
          lastTargetAtRef.current = Date.now();
          options.onBasePositionChange?.([displayX, displayY]);
          await showPromptButton(displayX, displayY);
        } else if (
          app &&
          app.name === "Prompt Picker" &&
          lastTargetAtRef.current > 0 &&
          lastButtonPositionRef.current
        ) {
          // Grace period during overlay self-interaction
          const recentlyHadTarget = Date.now() - lastTargetAtRef.current < 3000;
          if (recentlyHadTarget) {
            const [x, y] = lastButtonPositionRef.current;
            await showPromptButton(x, y);
            setTimeout(poll, 500 + Math.random() * 500);
            return;
          }
        }

        // No input target and no recent target — show at default/fallback position
        // This ensures the button appears even on first run before any target is detected
        if (floatingButtonVisible && lastButtonPositionRef.current) {
          const [x, y] = lastButtonPositionRef.current;
          await showPromptButton(x, y);
          setShowAttached(false);
        }

        setTimeout(poll, 1000 + Math.random() * 1000);
      } catch {
        setTimeout(poll, 2000);
      }
    };

    prevVisibilityRef.current = floatingButtonVisible;
    if (!floatingButtonVisible) {
      hidePromptButton().catch(console.error);
      hidePromptPopover().catch(console.error);
      return;
    }

    poll();

    return () => {
      activeRef.current = false;
    };
  }, [
    blacklist.join("\u0000"),
    overlayPlacement.buttonOffset
      ? `${overlayPlacement.buttonOffset.x}:${overlayPlacement.buttonOffset.y}`
      : "none",
    floatingButtonVisible,
  ]);

  return { target, showAttached };
}
