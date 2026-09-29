package io.sujiu.app

import android.app.Activity
import android.os.Bundle
import android.view.Gravity
import android.widget.LinearLayout
import android.widget.TextView

class MainActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val content = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(48, 48, 48, 48)

            addView(TextView(context).apply {
                text = "Sujiu"
                textSize = 32f
                gravity = Gravity.CENTER
            })

            addView(TextView(context).apply {
                text = "Android shell · Rust core wiring comes next"
                textSize = 16f
                gravity = Gravity.CENTER
            })
        }

        setContentView(content)
    }
}
