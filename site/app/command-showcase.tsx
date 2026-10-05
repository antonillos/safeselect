import gallery from "./cli-gallery";
import { sitePath } from "./shared";

const showcaseId = "command-showcase";

const selectionStyles = gallery.groups
  .flatMap((group) => [
    `.showcase-tabs:has(#${showcaseId}-group-${group.id}:checked) ~ .showcase-panels .showcase-panel-${group.id} { display: block; }`,
    ...group.commands.map(
      (item) =>
        `.showcase-panel-${group.id}:has(#${showcaseId}-command-${group.id}-${item.id}:checked) .showcase-slide-${item.id} { display: block; }`,
    ),
  ])
  .join("\n");

export default function CommandShowcase() {
  return (
    <div className="showcase">
      <style>{selectionStyles}</style>
      <div className="showcase-tabs" role="radiogroup" aria-label="Command categories">
        {gallery.groups.map((item, index) => (
          <div key={item.id} className="showcase-tab-option">
            <input
              className="showcase-radio showcase-category-radio"
              id={`${showcaseId}-group-${item.id}`}
              name={`${showcaseId}-group`}
              type="radio"
              defaultChecked={index === 0}
            />
            <label
              className={`showcase-tab showcase-tab-${item.id}`}
              htmlFor={`${showcaseId}-group-${item.id}`}
            >
              <span>0{index + 1}</span>
              {item.id === "query" ? "Explore · SQL" : item.title}
            </label>
          </div>
        ))}
      </div>
      <div className="showcase-panels">
        {gallery.groups.map((current) => (
          <section
            key={current.id}
            className={`showcase-panel showcase-panel-${current.id}`}
            aria-label={current.title}
          >
            <div className="showcase-commands" aria-label="Choose a command">
              {current.commands.map((item, index) => (
                <div key={item.id} className="showcase-command-option">
                  <input
                    className="showcase-radio showcase-command-radio"
                    id={`${showcaseId}-command-${current.id}-${item.id}`}
                    name={`${showcaseId}-command-${current.id}`}
                    type="radio"
                    defaultChecked={index === 0}
                  />
                  <label
                    className={`showcase-command showcase-command-${item.id}`}
                    htmlFor={`${showcaseId}-command-${current.id}-${item.id}`}
                  >
                    {item.id}
                  </label>
                </div>
              ))}
            </div>
            <div className="showcase-slides">
              {current.commands.map((item) => (
                <div key={item.id} className={`showcase-slide showcase-slide-${item.id}`}>
                  <div className="showcase-stage">
                    <div className="showcase-copy">
                      <p className="eyebrow">{current.title}</p>
                      <h3>{item.id}</h3>
                      <p>{item.purpose}</p>
                      <code>{item.example}</code>
                      <p className="showcase-caption">{item.caption}</p>
                    </div>
                    <a
                      className="showcase-image"
                      href={sitePath(`/${item.image}`)}
                      target="_blank"
                      rel="noreferrer"
                      aria-label={`Enlarge ${item.id} terminal capture`}
                    >
                      <img
                        src={sitePath(`/${item.image}`)}
                        alt={`${item.id}: ${item.caption}`}
                        width="1440"
                        height="760"
                        loading="lazy"
                      />
                      <span>View full size ↗</span>
                    </a>
                  </div>
                </div>
              ))}
            </div>
            <div className="showcase-controls">
              <span>{current.commands.length} captures · Select a command above</span>
            </div>
          </section>
        ))}
      </div>
    </div>
  );
}
