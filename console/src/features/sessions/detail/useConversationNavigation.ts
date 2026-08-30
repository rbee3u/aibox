import { useCallback, useEffect, useRef, useState, type UIEvent } from "react";

import { conversationIsAwayFromLatest } from "@/features/sessions/detail/sessionFormat";

interface ConversationNavigationOptions {
  active: boolean;
  currentSessionKey?: string;
  detailRevision: number;
  loading: boolean;
}

/** Owns Conversation DOM scrolling, anchors, and the active user-message projection. */
export function useConversationNavigation({
  active,
  currentSessionKey,
  detailRevision,
  loading,
}: ConversationNavigationOptions) {
  const [showJumpLatest, setShowJumpLatest] = useState(false);
  const [activeUserMessage, setActiveUserMessage] = useState<string | null>(null);
  const conversationScrollRef = useRef<HTMLDivElement>(null);
  const userMessageRefs = useRef(new Map<string, HTMLElement>());

  const clear = useCallback(() => {
    setActiveUserMessage(null);
    setShowJumpLatest(false);
    userMessageRefs.current.clear();
  }, []);

  function onConversationScroll(event: UIEvent<HTMLDivElement>) {
    const element = event.currentTarget;
    setShowJumpLatest(conversationIsAwayFromLatest(element));
    const threshold = element.scrollTop + Math.min(element.clientHeight * 0.28, 180);
    let current: string | null = null;
    for (const [entryId, message] of userMessageRefs.current) {
      if (message.offsetTop <= threshold) current = entryId;
      else break;
    }
    if (current) setActiveUserMessage(current);
  }

  function jumpToLatest() {
    const element = conversationScrollRef.current;
    if (!element) return;
    if (typeof element.scrollTo === "function") {
      element.scrollTo({ top: element.scrollHeight, behavior: "smooth" });
    } else {
      element.scrollTop = element.scrollHeight;
    }
    setShowJumpLatest(false);
  }

  function jumpToUserMessage(entryId: string) {
    const container = conversationScrollRef.current;
    const message = userMessageRefs.current.get(entryId);
    if (!container || !message) return;
    const top = Math.max(0, message.offsetTop - 24);
    if (typeof container.scrollTo === "function") {
      container.scrollTo({ top, behavior: "smooth" });
    } else {
      container.scrollTop = top;
    }
    setActiveUserMessage(entryId);
  }

  function registerUserMessage(entryId: string, element: HTMLElement | null) {
    if (element) userMessageRefs.current.set(entryId, element);
    else userMessageRefs.current.delete(entryId);
  }

  useEffect(() => {
    if (!currentSessionKey) return;
    const frame = window.requestAnimationFrame(() => {
      const element = conversationScrollRef.current;
      if (element && typeof element.scrollTo === "function") {
        element.scrollTo({ top: 0, behavior: "auto" });
      } else if (element) {
        element.scrollTop = 0;
      }
      setShowJumpLatest(false);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [currentSessionKey]);

  useEffect(() => {
    if (!currentSessionKey || !active || loading) return;
    const frame = window.requestAnimationFrame(() => {
      const element = conversationScrollRef.current;
      if (element) setShowJumpLatest(conversationIsAwayFromLatest(element));
    });
    return () => window.cancelAnimationFrame(frame);
  }, [active, currentSessionKey, detailRevision, loading]);

  return {
    activeUserMessage,
    clear,
    conversationScrollRef,
    jumpToLatest,
    jumpToUserMessage,
    onConversationScroll,
    registerUserMessage,
    showJumpLatest,
  };
}
