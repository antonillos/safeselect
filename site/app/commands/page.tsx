import CommandShowcase from "../command-showcase";
import { Shell, sitePath, REPO } from "../shared";
import gallery from "../cli-gallery";

export const metadata = {
  title: "SafeSelect CLI command gallery",
  description:
    "A visual, grouped guide to SafeSelect CLI commands for PostgreSQL and MongoDB projects.",
};

export default function Commands() {
  return (
    <Shell>
      <article className="article cli-gallery">
        <p className="eyebrow">CLI / VISUAL GUIDE</p>
        <h1>{gallery.title}</h1>
        <p className="article-lede">{gallery.intro}</p>
        <p>
          Every image comes from the SafeSelect CLI or the disposable demo
          fixture. Use the command text below the image as the copyable version;
          the output is the part worth studying.
        </p>
        <CommandShowcase />
        <details><summary>Browse the complete command reference</summary>
        <nav className="gallery-nav" aria-label="Command groups">
          {gallery.groups.map((group) => (
            <a key={group.id} href={`#${group.id}`}>
              {group.title}
            </a>
          ))}
        </nav>
        {gallery.groups.map((group) => (
          <section className="command-group" id={group.id} key={group.id}>
            <p className="eyebrow">{group.id}</p>
            <h2>{group.title}</h2>
            <p>{group.description}</p>
            <div className="command-grid">
              {group.commands.map((item) => (
                <article className="command-card" key={item.id}>
                  <div className="command-card-heading">
                    <span className="command-id">{item.id}</span>
                    <h3>{item.purpose}</h3>
                  </div>
                  <figure>
                    <img
                      src={sitePath(`/${item.image}`)}
                      alt={`Terminal capture for ${item.command}`}
                      loading="lazy"
                      width="1440"
                      height="760"
                    />
                    <figcaption>{item.caption}</figcaption>
                  </figure>
                  <p className="command-example">
                    <code>{item.example}</code>
                  </p>
                </article>
              ))}
            </div>
          </section>
        ))}
        </details>
        <aside className="command-callout">
          <strong>Convention before configuration.</strong> In a project with
          one environment, SafeSelect infers the repository and environment.
          Add <code>--project</code> or <code>--environment</code> deliberately
          when a script must target something else.
        </aside>
        <p>
          Need the complete contract? Read the{" "}
          <a href={`${REPO}/blob/develop/src/cli.rs`}>CLI source</a> or return
          to the <a href={sitePath("/")}>SafeSelect overview</a>.
        </p>
      </article>
    </Shell>
  );
}
