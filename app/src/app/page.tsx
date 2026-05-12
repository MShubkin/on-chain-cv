// Главная страница — лендинг с объяснением концепции и тремя свойствами протокола.
// Никакого состояния и wallet-контекста — чистый серверный компонент.
export default function Home() {
  return (
    <div className="flex flex-col gap-10">
      {/* Заголовок + краткое описание */}
      <div className="flex flex-col gap-4">
        <div className="text-xs font-mono text-purple-400 uppercase tracking-widest">
          Solana · MPL-Core · Soulbound
        </div>
        <h1 className="text-4xl font-bold tracking-tight">
          Verified credentials,
          <br />
          impossible to fake.
        </h1>
        <p className="text-gray-400 text-lg leading-relaxed max-w-xl">
          Employers issue NFT-backed credentials directly on-chain. Candidates
          build a portfolio that any recruiter can verify in seconds — no
          screenshots, no PDFs, no trust required.
        </p>
      </div>

      {/* Три карточки: soulbound, burn on revoke, one link.
          Рендерятся через map — добавить четвёртую карточку можно одной строкой в массиве. */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        {[
          {
            icon: "🔒",
            title: "Soulbound",
            desc: "Credentials are frozen — recipients can't transfer or sell them.",
          },
          {
            icon: "🔥",
            title: "Burn on revoke",
            desc: "Revoked credentials disappear from the wallet instantly.",
          },
          {
            icon: "🔗",
            title: "One link",
            desc: "Recruiters verify on-chain data with a single URL.",
          },
        ].map((f) => (
          <div
            key={f.title}
            className="rounded-xl border border-gray-800 bg-gray-900 p-5 flex flex-col gap-2"
          >
            <span className="text-2xl">{f.icon}</span>
            <h3 className="font-semibold">{f.title}</h3>
            <p className="text-sm text-gray-400">{f.desc}</p>
          </div>
        ))}
      </div>

      {/* CTA — ведёт на /admin для первоначальной настройки платформы */}
      <div className="flex gap-4">
        <a
          href="/admin"
          className="rounded-lg bg-purple-600 hover:bg-purple-500 transition-colors px-5 py-2.5 text-sm font-medium"
        >
          Initialize Platform →
        </a>
      </div>
    </div>
  );
}
