plugins {
    kotlin("jvm") version "2.1.10"
}

group = "ai.muonium"
version = file("../VERSION").readText().trim()

repositories {
    mavenCentral()
}

kotlin {
    jvmToolchain(21)
}

val generatedSrcDir = layout.buildDirectory.dir("generated/kotlin")

val generateVersion by tasks.registering {
    val versionFile = file("../VERSION")
    val outputDir = generatedSrcDir
    inputs.file(versionFile)
    outputs.dir(outputDir)
    doLast {
        val ver = versionFile.readText().trim()
        val parts = ver.split(".")
        val dir = outputDir.get().asFile.resolve("ai/muonium/muongit")
        dir.mkdirs()
        dir.resolve("GeneratedVersion.kt").writeText(
            """package ai.muonium.muongit
            |
            |/** Auto-generated from VERSION file — do not edit */
            |internal object GeneratedVersion {
            |    const val STRING = "$ver"
            |    const val MAJOR = ${parts[0]}
            |    const val MINOR = ${parts[1]}
            |    const val PATCH = ${parts[2]}
            |}
            |""".trimMargin()
        )
    }
}

sourceSets {
    main {
        kotlin.srcDir("src/main/kotlin")
        kotlin.srcDir(generatedSrcDir)
    }
    test {
        kotlin.srcDir("src/test/kotlin")
    }
}

tasks.named("compileKotlin") { dependsOn(generateVersion) }

dependencies {
    testImplementation(kotlin("test"))
}

tasks.test {
    useJUnitPlatform()
}
