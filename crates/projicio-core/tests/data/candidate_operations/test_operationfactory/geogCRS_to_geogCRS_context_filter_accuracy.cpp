TEST(operation, geogCRS_to_geogCRS_context_filter_accuracy) {
    auto authFactory =
        AuthorityFactory::create(DatabaseContext::create(), "EPSG");
    {
        auto ctxt =
            CoordinateOperationContext::create(authFactory, nullptr, 1.0);
        ctxt->setSpatialCriterion(
            CoordinateOperationContext::SpatialCriterion::PARTIAL_INTERSECTION);

        auto list = CoordinateOperationFactory::create()->createOperations(
            authFactory->createCoordinateReferenceSystem("4179"),
            authFactory->createCoordinateReferenceSystem("4258"), ctxt);
        ASSERT_EQ(list.size(), 1U);
        EXPECT_EQ(list[0]->getEPSGCode(), 1644); // Poland - 1m
    }
    {
        auto ctxt =
            CoordinateOperationContext::create(authFactory, nullptr, 0.9);
        ctxt->setSpatialCriterion(
            CoordinateOperationContext::SpatialCriterion::PARTIAL_INTERSECTION);

        auto list = CoordinateOperationFactory::create()->createOperations(
            authFactory->createCoordinateReferenceSystem("4179"),
            authFactory->createCoordinateReferenceSystem("4258"), ctxt);
        ASSERT_EQ(list.size(), 0U);
    }
}

