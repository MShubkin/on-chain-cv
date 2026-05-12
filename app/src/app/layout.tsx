import type { Metadata } from "next";
import { SolanaWalletProvider } from "@/components/WalletProvider";
// CSS от wallet-adapter — стили для WalletMultiButton и модального окна выбора кошелька
import "@solana/wallet-adapter-react-ui/styles.css";
import "./globals.css";

export const metadata: Metadata = {
  title: "OnChainCV",
  description: "Verified credentials on Solana",
};

// Корневой layout — оборачивает всё приложение в wallet-контекст.
// SolanaWalletProvider должен быть снаружи любой страницы, которая вызывает useWallet/useConnection.
export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en">
      <body className="min-h-screen bg-gray-950 text-gray-100">
        <SolanaWalletProvider>
          {/* Навбар: логотип + ссылки. Простой, без состояния. */}
          <nav className="border-b border-gray-800 px-6 py-4 flex items-center justify-between">
            <a href="/" className="text-lg font-bold tracking-tight">
              OnChainCV
            </a>
            <div className="flex gap-6 text-sm text-gray-400">
              <a href="/" className="hover:text-white transition-colors">
                Home
              </a>
              <a href="/issuers" className="hover:text-white transition-colors">
                Issuers
              </a>
              <a href="/dashboard" className="hover:text-white transition-colors">
                Dashboard
              </a>
              <a href="/admin" className="hover:text-white transition-colors">
                Admin
              </a>
              <a href="/my-endorsements" className="hover:text-white transition-colors">
                My Endorsements
              </a>
            </div>
          </nav>
          {/* Контент страницы — ограничен max-w-3xl для читаемости текста */}
          <main className="max-w-3xl mx-auto px-6 py-12">{children}</main>
        </SolanaWalletProvider>
      </body>
    </html>
  );
}
