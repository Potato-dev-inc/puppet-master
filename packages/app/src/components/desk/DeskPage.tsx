// My computers (Labs): the page shell. The screens, keys and pairing come in later phases.
import '../../styles/cockpit-desk.css';

export function DeskPage() {
  return (
    <section className="ck-desk" aria-labelledby="ck-desk-title">
      <header className="ck-desk-head">
        <h2 id="ck-desk-title" className="ck-desk-title">
          My computers
        </h2>
        <p className="ck-desk-line">Share your mouse, keyboard and clipboard between your computers.</p>
      </header>
      <div className="ck-desk-empty" role="status">
        <p>Coming next: move between screens on this computer.</p>
      </div>
    </section>
  );
}
