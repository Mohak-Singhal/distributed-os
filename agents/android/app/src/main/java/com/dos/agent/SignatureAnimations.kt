package com.dos.agent

import android.animation.AnimatorSet
import android.animation.ObjectAnimator
import android.animation.PropertyValuesHolder
import android.view.View
import android.view.animation.AccelerateInterpolator
import android.view.animation.DecelerateInterpolator

/**
 * Signature interaction system — the "Lock-On" motif extended across the app.
 *
 * === Visual Language ===
 * "This app connects things in a physical, visible way."
 *
 * The core motif: pulse → lock → flow → resolve
 *
 *   1. lockOn()      — discovery: rings collapse, orb acknowledges (peer selected)
 *   2. transferEmit()  — sending: orb emits a directional pulse toward the selected peer
 *   3. transferResolve()  — complete: energy resolves outward, felt as a gentle ripple
 *
 * These three moments create a subconscious narrative:
 *   "I found it → I'm sending → It arrived"
 */
object SignatureAnimations {

    /**
     * Radar lock-on (when user taps a peer during scanning).
     * Three pulse rings collapse inward, orb bounces acknowledgment.
     */
    fun lockOn(
        orbButton: View,
        radarPulse1: View,
        radarPulse2: View,
        radarPulse3: View,
        onComplete: () -> Unit
    ) {
        val collapse = AnimatorSet().apply {
            playTogether(
                pulseCollapseAnim(radarPulse1, 0),
                pulseCollapseAnim(radarPulse2, 80),
                pulseCollapseAnim(radarPulse3, 160)
            )
        }

        val orbPulse = ObjectAnimator.ofPropertyValuesHolder(
            orbButton,
            PropertyValuesHolder.ofFloat(View.SCALE_X, 1f, 1.15f, 1f),
            PropertyValuesHolder.ofFloat(View.SCALE_Y, 1f, 1.15f, 1f)
        ).apply {
            duration = Anim.successBumpMs
            interpolator = Anim.overshoot
        }

        AnimatorSet().apply {
            playSequentially(collapse, orbPulse)
            addListener(object : android.animation.AnimatorListenerAdapter() {
                override fun onAnimationEnd(animation: android.animation.Animator) {
                    listOf(radarPulse1, radarPulse2, radarPulse3).forEach {
                        it.visibility = View.GONE
                        it.scaleX = 1f
                        it.scaleY = 1f
                        it.alpha = 0.4f
                    }
                    onComplete()
                }
            })
            start()
        }
    }

    /**
     * Transfer emission (when a file starts sending).
     * The target peer card pulses — a brief cyan glow to signal "energy flowing."
     */
    fun transferEmit(targetView: View) {
        val pulseUp = ObjectAnimator.ofFloat(targetView, View.SCALE_X, 1f, 1.04f).apply {
            duration = 120
            interpolator = DecelerateInterpolator()
        }
        val pulseDown = ObjectAnimator.ofFloat(targetView, View.SCALE_X, 1.04f, 1f).apply {
            duration = 200
            interpolator = Anim.overshoot
        }
        val glow = ObjectAnimator.ofFloat(targetView, View.ALPHA, 1f, 0.92f, 1f).apply {
            duration = 320
        }
        AnimatorSet().apply {
            playTogether(
                AnimatorSet().apply { playSequentially(pulseUp, pulseDown) },
                glow
            )
            start()
        }
    }

    /**
     * Transfer resolution (when a transfer completes).
     * A subtle scale ripple — like a stone dropped in water.
     */
    fun transferResolve(targetView: View) {
        val rippleX = ObjectAnimator.ofFloat(targetView, View.SCALE_X, 1f, 1.06f, 1f)
        val rippleY = ObjectAnimator.ofFloat(targetView, View.SCALE_Y, 1f, 1.06f, 1f)
        AnimatorSet().apply {
            playTogether(rippleX, rippleY)
            duration = Anim.successBumpMs
            interpolator = Anim.overshoot
            start()
        }
    }

    private fun pulseCollapseAnim(view: View, delayMs: Long) = ObjectAnimator.ofPropertyValuesHolder(
        view,
        PropertyValuesHolder.ofFloat(View.SCALE_X, view.scaleX, 0.3f),
        PropertyValuesHolder.ofFloat(View.SCALE_Y, view.scaleY, 0.3f),
        PropertyValuesHolder.ofFloat(View.ALPHA, view.alpha, 0f)
    ).apply {
        duration = 300
        startDelay = delayMs
        interpolator = AccelerateInterpolator()
    }
}
