"use client";

import { useEffect, useId, useState, useSyncExternalStore } from "react";
import gallery from "./cli-gallery";
import { sitePath } from "./shared";

const motionQuery = "(prefers-reduced-motion: reduce)";
function subscribeMotion(callback: () => void) {
  const query = window.matchMedia(motionQuery);
  query.addEventListener("change", callback);
  return () => query.removeEventListener("change", callback);
}

const slides = gallery.groups.flatMap((group, groupIndex) =>
  group.commands.map((command, slideIndex) => ({ command, groupIndex, slideIndex })));

export default function CommandShowcase() {
  const id = useId();
  const [active, setActive] = useState(0);
  const { groupIndex: group, slideIndex: slide } = slides[active];
  const [requestedPlay, setPlaying] = useState(true);
  const reducedMotion = useSyncExternalStore(subscribeMotion, () => window.matchMedia(motionQuery).matches, () => true);
  const playing = requestedPlay && !reducedMotion;
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const current = gallery.groups[group];
  const command = current.commands[slide];
  useEffect(() => {
    if (!playing || hovered || focused) return;
    const timer = window.setInterval(() => {
      if (!document.hidden) setActive((value) => (value + 1) % slides.length);
    }, 7000);
    return () => window.clearInterval(timer);
  }, [playing, hovered, focused]);
  function selectGroup(index: number) {
    setActive(slides.findIndex((item) => item.groupIndex === index));
    setPlaying(false);
  }
  function move(offset: number) {
    setActive((value) => (value + offset + slides.length) % slides.length);
    setPlaying(false);
  }
  return (
    <div className="showcase" onMouseEnter={() => setHovered(true)} onMouseLeave={() => setHovered(false)}
      onFocus={() => setFocused(true)} onBlur={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false); }}>
      <div className="showcase-tabs" role="tablist" aria-label="Command categories">
        {gallery.groups.map((item, index) => (
          <button key={item.id} id={`${id}-tab-${index}`} role="tab" aria-selected={group === index}
            aria-controls={`${id}-panel`} tabIndex={group === index ? 0 : -1}
            onClick={() => selectGroup(index)} onKeyDown={(event) => {
              let next = index;
              if (event.key === "ArrowRight") next = (index + 1) % gallery.groups.length;
              else if (event.key === "ArrowLeft") next = (index - 1 + gallery.groups.length) % gallery.groups.length;
              else if (event.key === "Home") next = 0;
              else if (event.key === "End") next = gallery.groups.length - 1;
              else return;
              event.preventDefault(); selectGroup(next);
              document.getElementById(`${id}-tab-${next}`)?.focus();
            }}><span>0{index + 1}</span>{item.id === "query" ? "Explore · SQL" : item.title}</button>
        ))}
      </div>
      <div id={`${id}-panel`} role="tabpanel" aria-labelledby={`${id}-tab-${group}`}>
        <div className="showcase-commands" aria-label="Choose a command">
          {current.commands.map((item, index) => <button key={item.id} aria-pressed={slide === index}
            onClick={() => { setActive(slides.findIndex((item) => item.groupIndex === group && item.slideIndex === index)); setPlaying(false); }}>{item.id}</button>)}
        </div>
        <div className="showcase-stage">
          <div className="showcase-copy">
            <p className="eyebrow">{current.title}</p>
            <h3>{command.id}</h3>
            <p>{command.purpose}</p>
            <code>{command.example}</code>
            <p className="showcase-caption">{command.caption}</p>
          </div>
          <a className="showcase-image" href={sitePath(`/${command.image}`)} target="_blank" rel="noreferrer" aria-label={`Enlarge ${command.id} terminal capture`}>
            <img src={sitePath(`/${command.image}`)} alt={`${command.id}: ${command.caption}`} width="1440" height="760" />
            <span>View full size ↗</span>
          </a>
        </div>
        <div className="showcase-controls">
          <span>{String(slide + 1).padStart(2, "0")} / {String(current.commands.length).padStart(2, "0")} · {command.id}</span>
          <div>
            <button onClick={() => move(-1)} disabled={slides.length < 2} aria-label="Previous capture">←</button>
            <button onClick={() => setPlaying(!playing)} disabled={slides.length < 2} aria-pressed={playing}>{playing ? "Pause" : "Play"}</button>
            <button onClick={() => move(1)} disabled={slides.length < 2} aria-label="Next capture">→</button>
          </div>
        </div>
      </div>
      <noscript><p>Use the <a href={sitePath("/commands/")}>complete command reference</a> to browse all captures without JavaScript.</p></noscript>
    </div>
  );
}
