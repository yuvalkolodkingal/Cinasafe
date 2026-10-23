import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { Button } from './ui';

/** The 24 recovery words in a numbered grid, with Copy and Print. */
export function RecoveryWords({ words }: { words: string[] }) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  return (
    <div className="recovery">
      <ol className="word-grid" dir="ltr" lang="en">
        {words.map((w, i) => (
          <li key={i}>
            <span className="word-n">{i + 1}</span>
            <span className="word">{w}</span>
          </li>
        ))}
      </ol>
      <div className="button-row">
        <Button
          icon="copy"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(words.join(' '));
              setCopied(true);
              setTimeout(() => setCopied(false), 2000);
            } catch {
              /* clipboard unavailable */
            }
          }}
        >
          {copied ? t('common.copied') : t('common.copy')}
        </Button>
        <Button icon="print" onClick={() => window.print()}>
          {t('common.print')}
        </Button>
      </div>
    </div>
  );
}

export function StrengthMeter({ score }: { score: 0 | 1 | 2 | 3 | 4 }) {
  const { t } = useTranslation();
  return (
    <div className="strength" aria-live="polite">
      <div className="strength-bars" aria-hidden="true">
        {[1, 2, 3, 4].map((i) => (
          <span key={i} className={`strength-bar ${score >= i ? `on s${score}` : ''}`} />
        ))}
      </div>
      <span className={`strength-label s${score}`}>
        {t('wizard.strength.label')}: {t(`wizard.strength.${score}`)}
      </span>
    </div>
  );
}
