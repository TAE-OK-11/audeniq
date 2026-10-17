// Shapes of the hand-edited content/*.json files and the generated manifest.
// Types only: nothing here exists at runtime.
import type { TranslationMap } from './i18n.ts';

/** One entry of content/locales.json */
export interface Locale {
  code: string;
  lang: string;
  hreflang: string;
  /** "/" for the default language, otherwise "/<code>/" */
  prefix: string;
  ogLocale: string;
  /** Native language name shown in the language menu */
  name: string;
  dir: string;
  status: string;
  /** "system" skips the Pretendard web font */
  fonts?: string;
}

/** content/locales.json */
export interface LocaleRegistry {
  default: string;
  xDefault: string;
  locales: Locale[];
}

export interface Faq { q: string; a: string }
export interface GuideSection { heading: string; paragraphs: string[] }
export interface GuideSource { name: string; url: string }

interface GuidePageBase {
  path: string;
  title: string;
  description: string;
  question: string;
  answer: string;
  kind: string;
  serviceType?: string;
  audience?: string;
  facts: [string, string][];
  sections: GuideSection[];
  related: string[];
  sources: GuideSource[];
}
/** A guide page: its FAQ is either its own or the homepage FAQ (faqFromHome). */
export type GuidePage = GuidePageBase & ({ faqFromHome?: false; faq: Faq[] } | { faqFromHome: true; faq?: undefined });

/** A guide page with its FAQ filled in (homepage FAQ copied when faqFromHome) */
export type ResolvedGuidePage = GuidePageBase & { faqFromHome?: boolean; faq: Faq[] };

/** content/pages.json (Korean guide pages) */
export interface PagesFile {
  updated: string;
  pages: GuidePage[];
}

/** Homepage meta strings. title…ogImageAlt are required for every non-default language. */
export interface LocaleMeta {
  title?: string;
  description?: string;
  ogDescription?: string;
  ogImageAlt?: string;
  orgDescription: string;
  websiteDescription: string;
  languageLabel: string;
  languageNav: string;
}

export interface GuideLabel { name: string; eyebrow: string }

/** Guide page UI strings */
export interface GuideUi {
  skip: string;
  home: string;
  menuToggle: string;
  mainMenu: string;
  mobileMenu: string;
  contact: string;
  breadcrumb: string;
  status: string;
  official: string;
  overviewEyebrow: string;
  overviewTitle: string;
  overviewText: string;
  overviewCaption: string;
  detailsSuffix: string;
  faqEyebrow: string;
  faqTitle: string;
  faqText: string;
  relatedEyebrow: string;
  relatedTitle: string;
  preparing: string;
  more: string;
  sources: string;
  contactEyebrow: string;
  contactTitle: string;
  contactText: string;
  contactButton: string;
  contactNote: string;
  ogImageAlt: string;
  /** Only the Korean file has it (the RSS feed is Korean). */
  rssTitle?: string;
  guides: Record<string, GuideLabel>;
}

/** content/i18n/<code>.json */
export interface LocaleFile {
  updated: string;
  meta: LocaleMeta;
  /** Korean homepage segment → translation (not in the Korean file) */
  home?: TranslationMap;
  ui?: GuideUi;
  /** Guide pages in this language (Korean ones live in pages.json) */
  pages?: GuidePage[];
}

/** content/generated-manifest.json and public/site-version.json */
export interface SiteManifest {
  updated: string;
  hash: string;
  pages: string[];
  pageHashes: Record<string, string>;
}
