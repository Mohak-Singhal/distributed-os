package com.dos.agent

import android.animation.ValueAnimator
import android.content.Context
import android.graphics.Canvas
import android.graphics.Paint
import android.util.AttributeSet
import android.view.View
import android.view.animation.LinearInterpolator

/**
 * AirDrop-style radar scanner view.
 * Draws concentric rings that pulse outward from center,
 * with a sweeping "radar line" rotation.
 *
 * All configurable values come from [RadarConfig].
 */
class RadarScannerView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    private val density = resources.displayMetrics.density

    private val ringPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = 1.5f * density
        color = RadarConfig.radarColor
    }

    private val sweepPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
        color = RadarConfig.radarConeColor
    }

    private val dotPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
        color = RadarConfig.radarColor
    }

    private val centerPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.FILL
        color = RadarConfig.radarColor
    }

    private var sweepAngle = 0f
    private var pulsePhase = 0f

    private val sweepAnimator = ValueAnimator.ofFloat(0f, 360f).apply {
        duration = RadarConfig.sweepDurationMs
        repeatCount = ValueAnimator.INFINITE
        interpolator = LinearInterpolator()
        addUpdateListener {
            sweepAngle = it.animatedValue as Float
            invalidate()
        }
    }

    private val pulseAnimator = ValueAnimator.ofFloat(0f, 1f).apply {
        duration = RadarConfig.pulseDurationMs
        repeatCount = ValueAnimator.INFINITE
        interpolator = LinearInterpolator()
        addUpdateListener {
            pulsePhase = it.animatedValue as Float
            invalidate()
        }
    }

    var isScanning = false
        set(value) {
            field = value
            if (value) {
                sweepAnimator.start()
                pulseAnimator.start()
                visibility = VISIBLE
            } else {
                sweepAnimator.cancel()
                pulseAnimator.cancel()
                visibility = GONE
            }
        }

    init {
        visibility = GONE
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)

        val cx = width / 2f
        val cy = height / 2f
        val maxRadius = minOf(cx, cy) * 0.9f

        // Draw pulsing rings
        for (i in 0 until RadarConfig.ringCount) {
            val baseRadius = maxRadius * (RadarConfig.ringBaseRadiusFraction + i * RadarConfig.ringSpacingFraction)
            val pulseOffset = (pulsePhase + i * (1f / RadarConfig.ringCount)) % 1f
            val radius = baseRadius * (0.85f + pulseOffset * 0.3f)
            val alpha = ((1f - pulsePhase) * 180).toInt().coerceIn(0, 255)

            ringPaint.alpha = alpha
            canvas.drawCircle(cx, cy, radius, ringPaint)
        }

        // Draw sweep cone
        canvas.save()
        canvas.rotate(sweepAngle, cx, cy)
        canvas.drawArc(
            cx - maxRadius, cy - maxRadius,
            cx + maxRadius, cy + maxRadius,
            -RadarConfig.sweepConeAngle, RadarConfig.sweepConeAngle * 2, true, sweepPaint
        )
        canvas.restore()

        // Draw center dot
        canvas.drawCircle(cx, cy, RadarConfig.centerDotRadiusDp * density, centerPaint)

        // Draw orbiting dots
        for (i in 0 until RadarConfig.orbitingDotCount) {
            val angle = Math.toRadians((sweepAngle + i * (360.0 / RadarConfig.orbitingDotCount)))
            val dotRadius = maxRadius * RadarConfig.orbitRadiusFraction
            val dx = cx + (dotRadius * Math.cos(angle)).toFloat()
            val dy = cy + (dotRadius * Math.sin(angle)).toFloat()
            dotPaint.alpha = 200
            canvas.drawCircle(dx, dy, RadarConfig.orbitingDotRadiusDp * density, dotPaint)
        }
    }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        if (isScanning) {
            sweepAnimator.start()
            pulseAnimator.start()
        }
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        sweepAnimator.cancel()
        pulseAnimator.cancel()
    }
}
