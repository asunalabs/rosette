import type { LucideIcon } from "lucide-react";
import {
  ArrowUpRight,
  Bug,
  Check,
  EyeOff,
  GitBranch,
  KeyRound,
  LockKeyhole,
  Scale,
  Terminal,
} from "lucide-react";
import { Rosette } from "@/components/rosette";
import { ThemeToggle } from "@/components/theme-toggle";

const REPO_URL = "https://github.com/asunalabs/rosette";
const GETTING_STARTED_URL =
  "https://github.com/asunalabs/rosette/blob/master/docs/tutorials/getting-started.md";
const SECURITY_URL =
  "https://github.com/asunalabs/rosette/blob/master/SECURITY.md";
const LICENSE_URL =
  "https://github.com/asunalabs/rosette/blob/master/LICENSE";

// The product's own rosette — used as the nav/footer logo mark and the hero
// showcase. A fake fingerprint (this is a static marketing page), rendered
// by the same live algorithm a real contact's would be.
const BRAND_FINGERPRINT = "c47e1a9032f6b8d5e0a3";

/** "a3f21b9c44d2e07188aa" -> "a3f2 1b9c 44d2 e071 88aa" */
function formatFingerprint(hex: string): string {
  return hex.match(/.{1,4}/g)?.join(" ") ?? hex;
}

interface InfoCard {
  icon: LucideIcon;
  title: string;
  body: string;
  link?: { href: string; label: string };
}

const privacyFeatures: InfoCard[] = [
  {
    icon: LockKeyhole,
    title: "End-to-end encrypted with MLS",
    body: "Every conversation is secured with MLS (Messaging Layer Security) via OpenMLS — an IETF-standardized group-encryption protocol built for exactly this problem, not a homegrown cipher.",
  },
  {
    icon: EyeOff,
    title: "Relays can't see you",
    body: "The relay only stores and forwards padded ciphertext. It can't read names, message content, or who's in a conversation — blind by design, not by policy.",
  },
  {
    icon: KeyRound,
    title: "Phone required, never exposed",
    body: "Signup needs a phone number, verified once and immediately hashed — hidden by default and never shown to anyone. It isn't searchable unless you opt in, and there's no email to hand over either.",
  },
];

const statusItems: InfoCard[] = [
  {
    icon: GitBranch,
    title: "Working protocol skeleton",
    body: "The core protocol, relay, and CLI chat all run today, paired by QR code or contact link. The Kotlin Multiplatform app shell runs too — a polished consumer release doesn't exist yet.",
  },
  {
    icon: Bug,
    title: "Pre-audit",
    body: "The cryptography is real MLS via OpenMLS, but the implementation hasn't had an external audit yet. The audit gates public beta — don't stake your safety on it before then.",
    link: { href: SECURITY_URL, label: "Report a vulnerability" },
  },
  {
    icon: Scale,
    title: "AGPL-3.0, always",
    body: "Free and open source, including the relay and directory service. Copyleft means a hosted fork has to release its source too.",
    link: { href: LICENSE_URL, label: "Read the license" },
  },
];

const rosetteSamples: { fingerprint: string; verified?: boolean }[] = [
  { fingerprint: "a3f21b9c44d2e07188aa" },
  { fingerprint: "7c1e9f0a3b5d8e2f4160" },
  { fingerprint: "e04a8c2d91f637b0a5d2" },
  { fingerprint: "19bf76e2c8a04d5f3e91", verified: true },
  { fingerprint: "5d2f8a1c07e94b36d0f1" },
];

const footerLinks = [
  { href: REPO_URL, label: "GitHub" },
  { href: GETTING_STARTED_URL, label: "Getting started" },
  { href: LICENSE_URL, label: "License" },
  { href: SECURITY_URL, label: "Security" },
];

function FeatureCard({ icon: Icon, title, body, link }: InfoCard) {
  return (
    <div className="rounded-2xl bg-surface-2 p-6 sm:p-8">
      <div className="inline-flex h-11 w-11 items-center justify-center rounded-full bg-surface">
        <Icon className="h-5 w-5 text-accent" strokeWidth={2} aria-hidden="true" />
      </div>
      <h3 className="mt-5 text-lg font-semibold text-ink">{title}</h3>
      <p className="mt-2 text-base text-muted">{body}</p>
      {link && (
        <a
          href={link.href}
          target="_blank"
          rel="noopener noreferrer"
          className="mt-4 inline-flex items-center gap-1 text-sm font-semibold text-accent transition-colors duration-200 ease-out hover:text-accent-strong"
        >
          {link.label}
          <ArrowUpRight className="h-3.5 w-3.5" strokeWidth={2} aria-hidden="true" />
        </a>
      )}
    </div>
  );
}

export default function Home() {
  return (
    <>
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-50 focus:rounded-full focus:bg-accent focus:px-4 focus:py-2 focus:text-sm focus:font-semibold focus:text-on-accent"
      >
        Skip to content
      </a>

      <header className="sticky top-0 z-40 border-b border-hairline bg-bg">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-6 py-4">
          <a href="#" className="flex items-center gap-3">
            <Rosette fingerprint={BRAND_FINGERPRINT} size={32} />
            <span className="text-lg font-bold tracking-[-0.02em] text-ink">
              Rosette
            </span>
          </a>
          <div className="flex items-center gap-2 sm:gap-4">
            <a
              href={REPO_URL}
              target="_blank"
              rel="noopener noreferrer"
              className="hidden items-center gap-1.5 rounded-full px-4 py-2.5 text-sm font-semibold text-ink transition-colors duration-200 ease-out hover:bg-surface sm:inline-flex"
            >
              GitHub
              <ArrowUpRight className="h-4 w-4" strokeWidth={2} aria-hidden="true" />
            </a>
            <ThemeToggle />
          </div>
        </div>
      </header>

      <main id="main">
        {/* Hero */}
        <section
          aria-labelledby="hero-heading"
          className="mx-auto max-w-6xl px-6 pb-20 pt-16 sm:pt-24 md:pb-28 md:pt-28"
        >
          <div className="flex flex-col items-center gap-12 text-center md:flex-row md:items-center md:justify-between md:text-left">
            <div className="max-w-xl">
              <h1
                id="hero-heading"
                className="text-5xl font-bold tracking-[-0.02em] text-ink sm:text-6xl md:text-7xl"
              >
                Rosette
              </h1>
              <p className="mt-4 text-xl font-semibold text-ink sm:text-2xl">
                A private messenger for the Chat Control era.
              </p>
              <p className="mt-4 text-lg text-muted">
                End-to-end encrypted with MLS. Relays can&apos;t read your
                messages, your name, or who you talk to. Signup needs a
                phone number, verified once and hashed — never shown to
                anyone.
              </p>
              <div className="mt-8 flex flex-col items-center gap-3 sm:flex-row">
                <a
                  href={REPO_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex w-full items-center justify-center gap-2 rounded-full bg-accent px-6 py-3.5 text-base font-semibold text-on-accent transition-colors duration-200 ease-out hover:bg-accent-strong sm:w-auto"
                >
                  View on GitHub
                  <ArrowUpRight className="h-5 w-5" strokeWidth={2} aria-hidden="true" />
                </a>
                <a
                  href={GETTING_STARTED_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex w-full items-center justify-center gap-2 rounded-full border border-hairline bg-surface px-6 py-3.5 text-base font-semibold text-ink transition-colors duration-200 ease-out hover:bg-surface-2 sm:w-auto"
                >
                  <Terminal className="h-5 w-5" strokeWidth={2} aria-hidden="true" />
                  Try it locally
                </a>
              </div>
              <p className="mt-5 text-sm text-muted">
                Working protocol skeleton, pre-audit —{" "}
                <a
                  href="#status"
                  className="font-semibold text-ink underline underline-offset-4 transition-colors duration-200 ease-out hover:text-accent"
                >
                  what that means
                </a>
                .
              </p>
            </div>
            <Rosette
              fingerprint={BRAND_FINGERPRINT}
              size={64}
              className="h-40 w-40 shrink-0 sm:h-48 sm:w-48 md:h-56 md:w-56"
            />
          </div>
        </section>

        {/* Founder pledge */}
        <section aria-labelledby="pledge-heading" className="border-y border-hairline bg-surface">
          <div className="mx-auto max-w-4xl px-6 py-20 md:py-28">
            <h2 id="pledge-heading" className="sr-only">
              The founder&apos;s pledge
            </h2>
            <blockquote className="flex gap-6">
              <span aria-hidden="true" className="w-1 shrink-0 rounded-full bg-accent" />
              <p className="text-2xl font-semibold leading-snug tracking-[-0.01em] text-ink sm:text-3xl md:text-4xl">
                We would shut down before giving any government access to
                your data.
              </p>
            </blockquote>
            <p className="mt-6 pl-8 text-sm font-medium text-muted">— Rosette founder</p>
          </div>
        </section>

        {/* Why private */}
        <section
          aria-labelledby="why-private-heading"
          className="mx-auto max-w-6xl px-6 py-20 md:py-28"
        >
          <div className="max-w-2xl">
            <h2
              id="why-private-heading"
              className="text-3xl font-bold tracking-[-0.02em] text-ink sm:text-4xl"
            >
              Why private
            </h2>
            <p className="mt-4 text-lg text-muted">
              Privacy isn&apos;t a setting here. It&apos;s the architecture.
            </p>
          </div>
          <div className="mt-12 grid gap-6 md:grid-cols-3">
            {privacyFeatures.map((feature) => (
              <FeatureCard key={feature.title} {...feature} />
            ))}
          </div>
        </section>

        {/* The Rosette identicon, demonstrated live */}
        <section
          aria-labelledby="rosette-heading"
          className="border-y border-hairline bg-surface"
        >
          <div className="mx-auto max-w-6xl px-6 py-20 md:py-28">
            <div className="max-w-2xl">
              <h2
                id="rosette-heading"
                className="text-3xl font-bold tracking-[-0.02em] text-ink sm:text-4xl"
              >
                Hard to forge, easy to recognize
              </h2>
              <p className="mt-4 text-lg text-muted">
                Guilloché is the fine-line engraving on banknotes and
                passports — mathematically hard to reproduce, which is
                exactly why it stops forgery. Every contact&apos;s key
                fingerprint deterministically draws its own guilloché
                rosette. The same fingerprint always draws the same rosette,
                on your screen and theirs.
              </p>
            </div>
            <p className="mt-10 text-sm font-semibold text-muted">
              Five fingerprints. Five rosettes.
            </p>
            <div className="mt-4 grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-5">
              {rosetteSamples.map((sample) => (
                <div
                  key={sample.fingerprint}
                  className="flex flex-col items-center gap-3 rounded-2xl bg-surface-2 p-5"
                >
                  <Rosette
                    fingerprint={sample.fingerprint}
                    size={88}
                    verified={sample.verified}
                  />
                  <span className="font-mono text-xs text-muted">
                    {formatFingerprint(sample.fingerprint)}
                  </span>
                  {sample.verified && (
                    <span className="inline-flex items-center gap-1 text-xs font-semibold text-accent">
                      <Check className="h-3.5 w-3.5" strokeWidth={2} aria-hidden="true" />
                      Verified
                    </span>
                  )}
                </div>
              ))}
            </div>
            <p className="mt-8 max-w-2xl text-base text-muted">
              Verifying a contact — comparing safety numbers in person or
              over a trusted channel — engraves a second fine band around
              their rosette, like the one above. Unverified contacts are
              never marked unsafe; they just haven&apos;t been checked yet.
            </p>
          </div>
        </section>

        {/* Free forever */}
        <section
          aria-labelledby="free-forever-heading"
          className="mx-auto max-w-6xl px-6 py-20 md:py-28"
        >
          <div className="rounded-2xl bg-accent-soft px-6 py-12 sm:px-12 sm:py-16">
            <div className="mx-auto max-w-2xl text-center">
              <h2
                id="free-forever-heading"
                className="text-3xl font-bold tracking-[-0.02em] text-ink sm:text-4xl"
              >
                Free forever
              </h2>
              <p className="mt-4 text-lg text-ink">
                There is no paid tier, and there never will be. Billing
                requires an identity — a name on a card, an email on a
                receipt — and this app&apos;s entire premise is not having
                one.
              </p>
              <ul className="mt-8 flex flex-col items-center justify-center gap-3 text-base font-semibold text-ink sm:flex-row sm:gap-8">
                <li className="flex items-center gap-2">
                  <Check className="h-4 w-4 text-accent" strokeWidth={2} aria-hidden="true" />
                  No subscriptions
                </li>
                <li className="flex items-center gap-2">
                  <Check className="h-4 w-4 text-accent" strokeWidth={2} aria-hidden="true" />
                  No in-app purchases
                </li>
                <li className="flex items-center gap-2">
                  <Check className="h-4 w-4 text-accent" strokeWidth={2} aria-hidden="true" />
                  No premium tier
                </li>
              </ul>
            </div>
          </div>
        </section>

        {/* Status & transparency */}
        <section
          id="status"
          aria-labelledby="status-heading"
          className="scroll-mt-20 border-y border-hairline bg-surface"
        >
          <div className="mx-auto max-w-6xl px-6 py-20 md:py-28">
            <div className="max-w-2xl">
              <h2
                id="status-heading"
                className="text-3xl font-bold tracking-[-0.02em] text-ink sm:text-4xl"
              >
                Status &amp; transparency
              </h2>
              <p className="mt-4 text-lg text-muted">
                Rosette is a working protocol skeleton, not a finished
                product. What runs today is real; what doesn&apos;t exist
                yet is listed in the open, not hidden.
              </p>
            </div>
            <div className="mt-12 grid gap-6 md:grid-cols-3">
              {statusItems.map((item) => (
                <FeatureCard key={item.title} {...item} />
              ))}
            </div>
          </div>
        </section>
      </main>

      <footer className="bg-bg">
        <div className="mx-auto max-w-6xl px-6 py-12">
          <div className="flex flex-col items-center gap-8 sm:flex-row sm:items-start sm:justify-between">
            <a href="#" className="flex items-center gap-3">
              <Rosette fingerprint={BRAND_FINGERPRINT} size={28} />
              <div>
                <p className="text-base font-bold text-ink">Rosette</p>
                <p className="text-sm text-muted">
                  A private messenger for the Chat Control era.
                </p>
              </div>
            </a>
            <nav aria-label="Footer" className="flex flex-wrap items-center justify-center gap-x-8 gap-y-3">
              {footerLinks.map((link) => (
                <a
                  key={link.href}
                  href={link.href}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center gap-1 text-sm font-semibold text-ink transition-colors duration-200 ease-out hover:text-accent"
                >
                  {link.label}
                  <ArrowUpRight className="h-3.5 w-3.5" strokeWidth={2} aria-hidden="true" />
                </a>
              ))}
            </nav>
          </div>
          <div className="mt-10 border-t border-hairline pt-6 text-center text-sm text-muted sm:text-left">
            <p>© 2026 Rosette. Open source under AGPL-3.0.</p>
          </div>
        </div>
      </footer>
    </>
  );
}
