import { useRouter } from 'expo-router'
import { hasIdentity } from '../modules/spiritchat-crypto-core'

/**
 * After anything that can change which account (if any) is active on this
 * device — signing out, switching accounts, removing one, or adding a new
 * one — the screen stack up to this point may still reflect a now-stale
 * account. `dismissAll` collapses it before landing on the right
 * destination: without it, a later back-navigation (even something as
 * indirect as a swipe-back deep inside onboarding) could pop into a screen
 * still showing data from an account that's no longer active.
 *
 * Which destination is "right" depends on whether *some* account is still
 * active afterward — signing out of your only account goes to onboarding,
 * but signing out of one of several falls back to another automatically
 * (see IdentitySession.removeSlot's doc comment), and that should land in
 * the app, not onboarding.
 */
export function navigateAfterAccountChange(router: ReturnType<typeof useRouter>) {
  router.dismissAll()
  router.replace(hasIdentity() ? '/(tabs)/messages' : '/(onboarding)/welcome')
}
