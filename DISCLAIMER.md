# Disclaimer and acceptable use

svipall is a tool. Like any tool that speaks HTTP, it can be pointed at a system whose
owner did not invite it. What follows is not legal advice, and it is not part of the
licence — it states the terms on which this software is published, and where
responsibility sits.

Throughout this page, "the author" means ilien <contact@ilien.dev>, the copyright
holder of svipall.

## No warranty, no liability

svipall is provided **as is**, without warranty or condition of any kind, express or
implied. Sections 15, 16 and 17 of the GNU Affero General Public License version 3
(see [`LICENSE`](LICENSE)) govern, and nothing on this page enlarges them.

To the maximum extent permitted by law, the author is **not liable** for any damage,
loss, cost, fine, penalty, claim, investigation, prosecution, account suspension,
service ban, civil action or criminal proceeding arising from anyone's use of this
software, whatever its cause and whichever legal theory is invoked.

## Responsibility sits with the operator

You alone decide which addresses svipall connects to, how often, on whose behalf and
what you do with what comes back. That makes you — not the author, and not any
contributor — responsible for:

- complying with every law that applies to you and to the systems you reach, including
  computer-misuse and unauthorised-access statutes, copyright and database rights,
  competition law, contract law, and criminal law generally;
- complying with data-protection law when the pages you fetch contain personal data,
  including the GDPR, the UK GDPR, the CCPA/CPRA and their equivalents — lawful basis,
  minimisation, retention and the rights of data subjects are your obligations, not the
  tool's;
- the terms of service, acceptable-use policies, API terms, rate limits, contractual
  terms and access controls of every site and service you direct svipall at;
- the `robots.txt` directives of those sites, whatever weight the law of your
  jurisdiction gives them;
- obtaining any authorisation, licence, consent or contract required before you access
  a system, and being able to show it;
- what you do with the extracted data afterwards: storage, publication, resale,
  training, aggregation and onward transfer are your acts alone.

## No authorisation is granted or implied

Publishing this software grants you **no right, permission, authorisation or licence
with respect to any third-party system, network, account, dataset or service**. The
licence in [`LICENSE`](LICENSE) covers this source code and nothing else. That svipall
is technically capable of passing a bot-detection wall, solving a challenge, or
presenting a browser fingerprint is a statement about software, never a statement that
you are permitted to do so in a given case. Capability is not permission.

If you do not have authorisation for a target, this tool does not give it to you, and
using it there may be unlawful where you are.

## Intended use

svipall exists for work its operator is entitled to do: retrieving public information,
accessing your own accounts and systems, research and archiving, testing sites you own
or are engaged to test, and giving AI agents readable access to the open web under the
operator's own responsibility.

It is **not** published for, and its author does not support, unauthorised access to
systems, credential stuffing or account takeover, evading a ban or a paywall you are
subject to, scraping personal data without a lawful basis, denial of service or abusive
request volumes, fraud, spam, or defeating access controls that protect someone else's
private data.

Behaving as a good client is your job: request politely, back off when asked, identify
yourself where that is expected, cache instead of re-fetching, and stop when a site
tells you to. svipall gives you throttling, origin policy and robots handling to do
exactly that ([`README.md`](README.md), "Safety and privacy"); using them is your
choice.

## Captcha and challenge handling

svipall solves challenges locally, on the operator's own machine or by the operator's
own hand, for sessions the operator is entitled to conduct. Bypassing a challenge on a
system you have no right to access may be a criminal offence in your jurisdiction, may
breach a contract you accepted, and is your act, not the author's.

## Indemnity

If your use of svipall leads to a claim against the author — from a site operator, a
regulator, a data subject or anyone else — you will indemnify and hold the author
harmless for that claim, its defence and its costs.

## If you are unsure

Ask a qualified lawyer in your jurisdiction before you run it against a system that is
not yours. The right moment for that question is before the first request, not after
the letter arrives.
