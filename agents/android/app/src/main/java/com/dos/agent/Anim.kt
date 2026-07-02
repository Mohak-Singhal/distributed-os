package com.dos.agent

import android.view.animation.AccelerateDecelerateInterpolator
import android.view.animation.DecelerateInterpolator
import android.view.animation.OvershootInterpolator
import androidx.interpolator.view.animation.FastOutLinearInInterpolator
import androidx.interpolator.view.animation.FastOutSlowInInterpolator
import androidx.interpolator.view.animation.LinearOutSlowInInterpolator

/**
 * Centralized animation system.
 *
 * == Motion Language ==
 * Personality: "Soft but responsive" — slight elasticity, never mechanical.
 *   - Press: quick (80ms) with spring-back overshoot — feels tactile, not mushy
 *   - Transitions: 220–250ms FastOutSlowIn — relaxed but not slow
 *   - Success: bouncy (1.15x overshoot) — celebrates without being loud
 *   - Failure: oscillating shake (400ms) — conveys urgency without panic
 *
 * Rules:
 *   1. No linear animations — every motion has ease
 *   2. No durations < 60ms (too fast to register) or > 500ms (feels slow for UI)
 *   3. Transitions always slide 12dp + fade — direction adds spatial context
 *   4. Press always has spring overshoot — communicates "done" vs "in-progress"
 */
object Anim {

    // ── Durations ──────────────────────────────────────────────
    // All values in milliseconds. Range: 80–400ms for interactive,
    // up to 2000ms for auto-dismiss.
    const val pressMs = 80L
    const val releaseMs = 120L
    const val fadeInMs = 220L
    const val fadeOutMs = 180L
    const val slideMs = 220L
    const val listChangeMs = 240L
    const val bannerMs = 250L
    const val successBumpMs = 300L
    const val shakeMs = 400L
    const val tabSwitchMs = 250L
    const val progressBarMs = 400L
    const val indicatorMs = 300L
    const val radarPulseMs = 1800L
    const val flashMs = 600L
    const val autoDismissMs = 2000L

    // ── Interpolators ──────────────────────────────────────────
    val fastOutSlowIn = FastOutSlowInInterpolator()    // primary: smooth settling
    val linearOutSlowIn = LinearOutSlowInInterpolator() // exit: fast start, slow settle
    val fastOutLinearIn = FastOutLinearInInterpolator() // press: quick to max
    val overshoot = OvershootInterpolator(1.5f)         // spring-back after press
    val overshootStrong = OvershootInterpolator(2f)     // celebration / indicator spring
    val decelerate = DecelerateInterpolator()           // gentle stop
    val accelerateDecelerate = AccelerateDecelerateInterpolator() // symmetrical

    // ── Scale ─────────────────────────────────────────────────
    const val pressScale = 0.94f          // subtle, not squished
    const val hoverScale = 1.03f          // barely perceptible lift
    const val successBumpScale = 1.15f    // celebratory, not overwhelming

    // ── Translation ────────────────────────────────────────────
    const val slideDp = 12f               // standard directional hint
    const val swipeDismissThresholdDp = 80f  // natural swipe distance
}
