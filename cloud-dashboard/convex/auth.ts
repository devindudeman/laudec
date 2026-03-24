import GitHub from "@auth/core/providers/github";
import Google from "@auth/core/providers/google";
import { convexAuth } from "@convex-dev/auth/server";

export const { auth, signIn, signOut, store, isAuthenticated } = convexAuth({
  providers: [
    GitHub({
      profile(profile) {
        return {
          id: String(profile.id),
          name: profile.name ?? profile.login ?? undefined,
          email: profile.email ?? undefined,
          image: profile.avatar_url ?? undefined,
        };
      },
    }),
    Google,
  ],
});
